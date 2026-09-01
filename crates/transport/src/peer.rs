use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use tokio::sync::{mpsc, Mutex};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_gathering_state::RTCIceGatheringState;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::config::IceConfig;

pub struct PeerConnection {
    pc: Arc<RTCPeerConnection>,
    data_channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    incoming: mpsc::UnboundedReceiver<Vec<u8>>,
    ice_local_rx: Arc<Mutex<mpsc::UnboundedReceiver<String>>>,
    /// Remote trickle candidates received before setRemoteDescription (offerer side).
    pending_remote_candidates: Arc<Mutex<Vec<String>>>,
}

struct Inner {
    pc: Arc<RTCPeerConnection>,
    data_channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    incoming_tx: mpsc::UnboundedSender<Vec<u8>>,
    incoming_rx: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    ice_local_rx: Arc<Mutex<mpsc::UnboundedReceiver<String>>>,
    pending_remote_candidates: Arc<Mutex<Vec<String>>>,
}

impl PeerConnection {
    pub async fn send(&self, payload: &[u8]) -> Result<()> {
        let guard = self.data_channel.lock().await;
        let channel = guard
            .as_ref()
            .ok_or_else(|| anyhow!("data channel not ready"))?;
        channel
            .send(&Bytes::copy_from_slice(payload))
            .await
            .context("send on data channel")
            .map(|_| ())
    }

    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.incoming.recv().await
    }

    pub fn is_connected(&self) -> bool {
        matches!(
            self.pc.connection_state(),
            RTCPeerConnectionState::Connected
        ) || matches!(
            self.pc.ice_connection_state(),
            RTCIceConnectionState::Connected | RTCIceConnectionState::Completed
        )
    }

    pub async fn add_remote_candidate(&self, candidate_json: &str) -> Result<()> {
        if self.pc.remote_description().await.is_none() {
            self.pending_remote_candidates
                .lock()
                .await
                .push(candidate_json.to_string());
            return Ok(());
        }
        let init: RTCIceCandidateInit =
            serde_json::from_str(candidate_json).context("parse remote ice candidate")?;
        if init.candidate.is_empty() {
            self.pc
                .add_ice_candidate(RTCIceCandidateInit::default())
                .await
                .context("signal end-of-candidates")?;
            return Ok(());
        }
        self.add_ice_candidate_now(candidate_json).await
    }

    async fn add_ice_candidate_now(&self, candidate_json: &str) -> Result<()> {
        let init: RTCIceCandidateInit =
            serde_json::from_str(candidate_json).context("parse remote ice candidate")?;
        self.pc
            .add_ice_candidate(init)
            .await
            .context("add ice candidate")?;
        Ok(())
    }

    async fn flush_pending_remote_candidates(&self) {
        let pending: Vec<String> = {
            let mut guard = self.pending_remote_candidates.lock().await;
            std::mem::take(&mut *guard)
        };
        for candidate in pending {
            if let Ok(init) = serde_json::from_str::<RTCIceCandidateInit>(&candidate) {
                if init.candidate.is_empty() {
                    let _ = self
                        .pc
                        .add_ice_candidate(RTCIceCandidateInit::default())
                        .await;
                } else {
                    let _ = self.pc.add_ice_candidate(init).await;
                }
            }
        }
    }

    pub async fn drain_local_candidates(&self) -> Vec<String> {
        let mut rx = self.ice_local_rx.lock().await;
        let mut out = Vec::new();
        while let Ok(c) = rx.try_recv() {
            out.push(c);
        }
        out
    }

    pub async fn apply_remote_answer(&self, answer_sdp: &str) -> Result<()> {
        self.pc
            .set_remote_description(RTCSessionDescription::answer(answer_sdp.to_string())?)
            .await
            .context("set remote answer")?;
        self.flush_pending_remote_candidates().await;
        Ok(())
    }

    pub async fn apply_remote_offer(&self, offer_sdp: &str) -> Result<()> {
        self.pc
            .set_remote_description(RTCSessionDescription::offer(offer_sdp.to_string())?)
            .await
            .context("set remote offer")?;
        self.flush_pending_remote_candidates().await;
        Ok(())
    }

    pub async fn set_remote_answer(&self, answer_sdp: &str) -> Result<()> {
        self.pc
            .set_remote_description(RTCSessionDescription::answer(answer_sdp.to_string())?)
            .await
            .context("set remote answer")?;
        wait_until_connected(&self.pc).await
    }

    pub async fn wait_ready(&self) -> Result<()> {
        for _ in 0..450 {
            if self.data_channel.lock().await.is_some() {
                wait_until_connected(&self.pc).await?;
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Err(anyhow!("timed out waiting for data channel"))
    }

    pub async fn close(self) {
        let _ = self.pc.close().await;
    }
}

pub async fn create_offer(ice: &IceConfig) -> Result<String> {
    let inner = new_peer(ice).await?;
    let pc = Arc::clone(&inner.pc);

    pc.create_data_channel("corgigram", None)
        .await
        .context("create data channel")?;

    let offer = pc.create_offer(None).await.context("create offer")?;
    pc.set_local_description(offer)
        .await
        .context("set local offer")?;
    wait_for_ice_gathering(&pc, ice).await?;
    let offer_sdp = pc
        .local_description()
        .await
        .context("local offer missing")?
        .sdp;
    Ok(offer_sdp)
}

pub async fn create_answer(ice: &IceConfig, offer_sdp: &str) -> Result<String> {
    let inner = new_peer(ice).await?;
    let pc = Arc::clone(&inner.pc);

    let offer = RTCSessionDescription::offer(offer_sdp.to_string())?;
    pc.set_remote_description(offer)
        .await
        .context("set remote offer")?;

    let answer = pc.create_answer(None).await.context("create answer")?;
    pc.set_local_description(answer)
        .await
        .context("set local answer")?;
    wait_for_ice_gathering(&pc, ice).await?;
    let answer_sdp = pc
        .local_description()
        .await
        .context("local answer missing")?
        .sdp;
    Ok(answer_sdp)
}

pub async fn connect_as_offerer(
    ice: &IceConfig,
    offer_sdp: &str,
    answer_sdp: &str,
) -> Result<PeerConnection> {
    let inner = new_peer(ice).await?;
    let pc = Arc::clone(&inner.pc);

    let offer = RTCSessionDescription::offer(offer_sdp.to_string())?;
    pc.set_local_description(offer)
        .await
        .context("set local offer")?;

    let answer = RTCSessionDescription::answer(answer_sdp.to_string())?;
    pc.set_remote_description(answer)
        .await
        .context("set remote answer")?;

    wait_for_incoming_data_channel(&inner).await?;
    wait_until_connected(&pc).await?;
    Ok(inner.into_peer_connection())
}

pub async fn wait_for_incoming(
    ice: &IceConfig,
    offer_sdp: &str,
    answer_sdp: &str,
) -> Result<PeerConnection> {
    let inner = new_peer(ice).await?;
    let pc = Arc::clone(&inner.pc);

    let offer = RTCSessionDescription::offer(offer_sdp.to_string())?;
    pc.set_remote_description(offer)
        .await
        .context("set remote offer")?;

    let answer = RTCSessionDescription::answer(answer_sdp.to_string())?;
    pc.set_local_description(answer)
        .await
        .context("set local answer")?;

    wait_for_incoming_data_channel(&inner).await?;
    wait_until_connected(&pc).await?;
    Ok(inner.into_peer_connection())
}

impl Inner {
    async fn flush_pending_remote_candidates(&self) {
        let pending: Vec<String> = {
            let mut guard = self.pending_remote_candidates.lock().await;
            std::mem::take(&mut *guard)
        };
        for candidate in pending {
            if let Ok(init) = serde_json::from_str::<RTCIceCandidateInit>(&candidate) {
                if init.candidate.is_empty() {
                    let _ = self
                        .pc
                        .add_ice_candidate(RTCIceCandidateInit::default())
                        .await;
                } else {
                    let _ = self.pc.add_ice_candidate(init).await;
                }
            }
        }
    }

    fn into_peer_connection(mut self) -> PeerConnection {
        PeerConnection {
            pc: self.pc,
            data_channel: Arc::clone(&self.data_channel),
            incoming: self
                .incoming_rx
                .take()
                .expect("incoming receiver already taken"),
            ice_local_rx: Arc::clone(&self.ice_local_rx),
            pending_remote_candidates: Arc::clone(&self.pending_remote_candidates),
        }
    }
}

async fn new_peer(ice: &IceConfig) -> Result<Inner> {
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media_engine)?;

    let mut setting_engine = SettingEngine::default();
    ice.apply_setting_engine(&mut setting_engine);

    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .with_setting_engine(setting_engine)
        .build();

    let config = RTCConfiguration {
        ice_servers: ice.rtc_ice_servers(),
        ice_transport_policy: ice.ice_transport_policy(),
        ..Default::default()
    };

    let pc = Arc::new(api.new_peer_connection(config).await?);
    let data_channel = Arc::new(Mutex::new(None));
    let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
    let (ice_local_tx, ice_local_rx) = mpsc::unbounded_channel();
    let ice_local_rx = Arc::new(Mutex::new(ice_local_rx));
    let pending_remote_candidates = Arc::new(Mutex::new(Vec::new()));

    {
        let ice_local_tx = ice_local_tx.clone();
        pc.on_ice_candidate(Box::new(move |candidate| {
            let ice_local_tx = ice_local_tx.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(init) = candidate.to_json() {
                        if let Ok(json) = serde_json::to_string(&init) {
                            let _ = ice_local_tx.send(json);
                        }
                    }
                }
            })
        }));
    }

    {
        let data_channel = Arc::clone(&data_channel);
        let incoming_tx = incoming_tx.clone();
        pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let data_channel = Arc::clone(&data_channel);
            let incoming_tx = incoming_tx.clone();
            Box::pin(async move {
                wire_data_channel(&dc, incoming_tx.clone()).await;
                let mut guard = data_channel.lock().await;
                if guard.is_none() {
                    *guard = Some(dc);
                }
            })
        }));
    }

    Ok(Inner {
        pc,
        data_channel,
        incoming_tx,
        incoming_rx: Some(incoming_rx),
        ice_local_rx,
        pending_remote_candidates,
    })
}

async fn attach_data_channel(inner: &Inner, dc: Arc<RTCDataChannel>) {
    wire_data_channel(&dc, inner.incoming_tx.clone()).await;
    let mut guard = inner.data_channel.lock().await;
    if guard.is_none() {
        *guard = Some(dc);
    }
}

async fn wire_data_channel(dc: &Arc<RTCDataChannel>, incoming_tx: mpsc::UnboundedSender<Vec<u8>>) {
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let incoming_tx = incoming_tx.clone();
        Box::pin(async move {
            let _ = incoming_tx.send(msg.data.to_vec());
        })
    }));
}

async fn wait_for_incoming_data_channel(inner: &Inner) -> Result<()> {
    for _ in 0..150 {
        if inner.data_channel.lock().await.is_some() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Err(anyhow!("timed out waiting for incoming data channel"))
}

async fn wait_for_ice_gathering(pc: &Arc<RTCPeerConnection>, ice: &IceConfig) -> Result<()> {
    let needs_turn = ice.has_turn();
    let max_wait = if needs_turn { 600 } else { 100 };
    for _ in 0..max_wait {
        if pc.ice_gathering_state() == RTCIceGatheringState::Complete {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // Trickle ICE continues after SDP exchange; don't fail on slow TURN allocation.
    Ok(())
}

async fn wait_until_connected(pc: &Arc<RTCPeerConnection>) -> Result<()> {
    for _ in 0..450 {
        match pc.connection_state() {
            RTCPeerConnectionState::Connected => return Ok(()),
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                return Err(anyhow!("peer connection {:?}", pc.connection_state()));
            }
            _ => {}
        }

        let ice = pc.ice_connection_state();
        if ice == RTCIceConnectionState::Connected || ice == RTCIceConnectionState::Completed {
            return Ok(());
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Err(anyhow!("timed out waiting for peer connection"))
}

/// Full in-process WebRTC connect: offerer + answerer with in-memory SDP exchange.
pub async fn run_local_demo(ice: &IceConfig) -> Result<(PeerConnection, PeerConnection)> {
    let offerer_inner = new_peer(ice).await?;
    let offerer_pc = Arc::clone(&offerer_inner.pc);

    let dc = offerer_pc
        .create_data_channel("corgigram", None)
        .await
        .context("create data channel")?;
    attach_data_channel(&offerer_inner, dc).await;

    let offer = offerer_pc.create_offer(None).await.context("create offer")?;
    offerer_pc
        .set_local_description(offer)
        .await
        .context("set local offer")?;
    wait_for_ice_gathering(&offerer_pc, ice).await?;
    let offer_sdp = offerer_pc
        .local_description()
        .await
        .context("local offer missing")?
        .sdp;

    let answerer_inner = new_peer(ice).await?;
    let answerer_pc = Arc::clone(&answerer_inner.pc);

    answerer_pc
        .set_remote_description(RTCSessionDescription::offer(offer_sdp)?)
        .await
        .context("set remote offer")?;

    let answer = answerer_pc.create_answer(None).await.context("create answer")?;
    answerer_pc
        .set_local_description(answer)
        .await
        .context("set local answer")?;
    wait_for_ice_gathering(&answerer_pc, ice).await?;
    let answer_sdp = answerer_pc
        .local_description()
        .await
        .context("local answer missing")?
        .sdp;

    offerer_pc
        .set_remote_description(RTCSessionDescription::answer(answer_sdp)?)
        .await
        .context("set remote answer")?;

    wait_for_incoming_data_channel(&answerer_inner).await?;
    wait_until_connected(&offerer_pc).await?;
    wait_until_connected(&answerer_pc).await?;

    Ok((
        offerer_inner.into_peer_connection(),
        answerer_inner.into_peer_connection(),
    ))
}

/// Answerer role: import offer SDP on a live peer, produce answer SDP.
pub async fn run_answerer_role(ice: &IceConfig, offer_sdp: &str) -> Result<(PeerConnection, String)> {
    let inner = new_peer(ice).await?;
    let pc = Arc::clone(&inner.pc);

    pc.set_remote_description(RTCSessionDescription::offer(offer_sdp.to_string())?)
        .await
        .context("set remote offer")?;
    inner.flush_pending_remote_candidates().await;

    let answer = pc.create_answer(None).await.context("create answer")?;
    pc.set_local_description(answer)
        .await
        .context("set local answer")?;
    wait_for_ice_gathering(&pc, ice).await?;
    let answer_sdp = pc
        .local_description()
        .await
        .context("local answer missing")?
        .sdp;

    Ok((inner.into_peer_connection(), answer_sdp))
}

/// Offerer role: create offer on a live peer (keeps connection for later answer).
pub async fn run_offerer_role(ice: &IceConfig) -> Result<(PeerConnection, String)> {
    let inner = new_peer(ice).await?;
    let pc = Arc::clone(&inner.pc);

    let dc = pc
        .create_data_channel("corgigram", None)
        .await
        .context("create data channel")?;
    attach_data_channel(&inner, dc).await;

    let offer = pc.create_offer(None).await.context("create offer")?;
    pc.set_local_description(offer)
        .await
        .context("set local offer")?;
    wait_for_ice_gathering(&pc, ice).await?;
    let offer_sdp = pc
        .local_description()
        .await
        .context("local offer missing")?
        .sdp;

    Ok((inner.into_peer_connection(), offer_sdp))
}

/// Connect using pre-exchanged SDP files (matched offer + answer pair).
pub async fn connect_from_sdps(
    ice: &IceConfig,
    offer_sdp: &str,
    answer_sdp: &str,
) -> Result<(PeerConnection, PeerConnection)> {
    let offerer_inner = new_peer(ice).await?;
    let offerer_pc = Arc::clone(&offerer_inner.pc);

    offerer_pc
        .set_local_description(RTCSessionDescription::offer(offer_sdp.to_string())?)
        .await
        .context("offerer set local offer")?;

    let answerer_inner = new_peer(ice).await?;
    let answerer_pc = Arc::clone(&answerer_inner.pc);

    answerer_pc
        .set_remote_description(RTCSessionDescription::offer(offer_sdp.to_string())?)
        .await
        .context("answerer set remote offer")?;

    answerer_pc
        .set_local_description(RTCSessionDescription::answer(answer_sdp.to_string())?)
        .await
        .context("answerer set local answer")?;

    offerer_pc
        .set_remote_description(RTCSessionDescription::answer(answer_sdp.to_string())?)
        .await
        .context("offerer set remote answer")?;

    wait_for_incoming_data_channel(&answerer_inner).await?;
    wait_for_incoming_data_channel(&offerer_inner).await?;
    wait_until_connected(&offerer_pc).await?;
    wait_until_connected(&answerer_pc).await?;

    Ok((
        offerer_inner.into_peer_connection(),
        answerer_inner.into_peer_connection(),
    ))
}
