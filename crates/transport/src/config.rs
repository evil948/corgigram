use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice::mdns::MulticastDnsMode;
use webrtc::ice::network_type::NetworkType;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy;

#[derive(Clone, Debug)]
pub struct TurnCredentials {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

#[derive(Clone, Debug)]
pub struct IceConfig {
    pub stun_urls: Vec<String>,
    pub turn_servers: Vec<TurnCredentials>,
    pub ipv4_only: bool,
    /// Force traffic through TURN relay (needed for some strict NAT setups).
    pub relay_only: bool,
}

impl Default for IceConfig {
    fn default() -> Self {
        Self {
            stun_urls: vec![
                "stun:stun.l.google.com:19302".into(),
                "stun:stun1.l.google.com:19302".into(),
                "stun:stun.cloudflare.com:3478".into(),
            ],
            turn_servers: vec![TurnCredentials {
                urls: vec![
                    "turn:openrelay.metered.ca:443?transport=tcp".into(),
                    "turns:openrelay.metered.ca:443".into(),
                    "turn:openrelay.metered.ca:80".into(),
                    "turn:openrelay.metered.ca:443".into(),
                ],
                username: "openrelayproject".into(),
                credential: "openrelayproject".into(),
            }],
            ipv4_only: true,
            relay_only: false,
        }
    }
}

impl IceConfig {
    /// Host candidates only — for same-machine or LAN testing.
    pub fn localhost() -> Self {
        Self {
            stun_urls: vec![],
            turn_servers: vec![],
            ipv4_only: true,
            relay_only: false,
        }
    }

    pub fn has_turn(&self) -> bool {
        !self.turn_servers.is_empty()
    }

    pub fn add_turn_server(&mut self, server: TurnCredentials) {
        self.turn_servers.push(server);
    }

    pub fn apply_setting_engine(&self, settings: &mut SettingEngine) {
        settings.set_ice_multicast_dns_mode(MulticastDnsMode::Disabled);
        settings.set_ice_timeouts(
            Some(std::time::Duration::from_secs(30)),
            Some(std::time::Duration::from_secs(90)),
            Some(std::time::Duration::from_secs(2)),
        );
        if self.ipv4_only {
            settings.set_network_types(vec![NetworkType::Udp4, NetworkType::Tcp4]);
        }
    }

    pub fn ice_transport_policy(&self) -> RTCIceTransportPolicy {
        if self.relay_only && self.has_turn() {
            RTCIceTransportPolicy::Relay
        } else {
            RTCIceTransportPolicy::All
        }
    }

    pub fn rtc_ice_servers(&self) -> Vec<RTCIceServer> {
        let mut servers = Vec::new();

        if !self.stun_urls.is_empty() {
            servers.push(RTCIceServer {
                urls: self.stun_urls.clone(),
                username: String::new(),
                credential: String::new(),
            });
        }

        for turn in &self.turn_servers {
            if turn.urls.is_empty() {
                continue;
            }
            servers.push(RTCIceServer {
                urls: turn.urls.clone(),
                username: turn.username.clone(),
                credential: turn.credential.clone(),
            });
        }

        servers
    }
}
