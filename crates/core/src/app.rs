use std::collections::HashSet;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use corgigram_crypto::{
    decrypt_avatar, decrypt_mailbox, encrypt_avatar, encrypt_mailbox, Identity, PreKeyBundle,
    Session, SessionInitiator, SessionResponder,
};
use corgigram_protocol::{ChatPayload, WireMessage};
use corgigram_storage::{ContactRecord, MessageRecord, OutboxRecord, Storage};
use corgigram_transport::{run_answerer_role, run_offerer_role, IceConfig, PeerConnection};
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::chat_media::{
    self, bytes_to_payload, message_record_from_payload, payload_to_bytes,
    prefers_mailbox_delivery, save_payload_attachments,
};
use crate::config::AppConfig;
use crate::signaling::{FirebaseSignaling, MailboxEntry};
use crate::turn::fetch_elixir_webrtc_turn;

#[derive(Clone, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub user_id: String,
    pub display_name: String,
    pub bundle_json: String,
    pub safety_hint: String,
    pub avatar_data_url: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectOfferResult {
    pub offer_sdp: String,
    pub contact_id: String,
    pub auto_signaling: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectAnswerResult {
    pub answer_sdp: String,
    pub contact_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectDiagnose {
    pub my_user_id: String,
    pub contact_id: String,
    pub has_pending_offer: bool,
    pub has_pending_answer: bool,
    pub is_active: bool,
    pub has_firebase_offer_to_me: bool,
    pub has_firebase_answer_from_contact: bool,
    pub local_ice_to_contact: usize,
    pub remote_ice_from_contact: usize,
    pub turn_fetched: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectAutoResult {
    pub contact_id: String,
    pub connected: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InvitationInfo {
    pub from_user_id: String,
    pub display_name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub has_identity: bool,
    pub profile: Option<ProfileInfo>,
    pub contacts: Vec<ContactRecord>,
    pub pending_invitations: Vec<InvitationInfo>,
    pub connected_contact_id: Option<String>,
    pub connecting_contact_id: Option<String>,
    /// Contact whose chat is open — WebRTC connect is being attempted.
    pub wanted_contact_id: Option<String>,
    pub firebase_configured: bool,
    pub firebase_database_url: String,
    pub firebase_database_url_override: Option<String>,
    pub firebase_uses_default_url: bool,
    pub outbox_count: usize,
    pub contact_presence: HashMap<String, bool>,
}

#[derive(Debug, Default)]
pub struct BackgroundTickResult {
    pub messages: Vec<MessageRecord>,
    pub contacts_changed: bool,
    pub status_updates: Vec<(String, String)>,
    pub connecting: bool,
}

pub use chat_media::{AttachmentData, OutgoingAttachment};

struct PendingOffer {
    contact_id: String,
    peer: PeerConnection,
    seen_ice: HashSet<String>,
    started_at: DateTime<Utc>,
}

struct PendingAnswer {
    contact_id: String,
    peer: PeerConnection,
    seen_ice: HashSet<String>,
    started_at: DateTime<Utc>,
}

struct ActiveChat {
    contact_id: String,
    peer: PeerConnection,
    session: Session,
}

pub struct CorgigramApp {
    data_dir: PathBuf,
    config: AppConfig,
    storage: Storage,
    identity: Option<Identity>,
    active: Arc<AsyncMutex<Option<ActiveChat>>>,
    /// Updated alongside `active` so snapshot works while poll holds the async lock.
    live_contact_id: Arc<RwLock<Option<String>>>,
    wanted_contact_id: Arc<RwLock<Option<String>>>,
    pending_offer: Arc<AsyncMutex<Option<PendingOffer>>>,
    pending_answer: Arc<AsyncMutex<Option<PendingAnswer>>>,
    pending_invitations: Arc<RwLock<Vec<InvitationInfo>>>,
    contact_presence: Arc<RwLock<HashMap<String, bool>>>,
    last_presence_heartbeat: Arc<AsyncMutex<Option<DateTime<Utc>>>>,
    last_presence_sync: Arc<AsyncMutex<Option<DateTime<Utc>>>>,
    firebase_client: Arc<Mutex<Option<FirebaseSignaling>>>,
    connect_backoff: Arc<AsyncMutex<HashMap<String, DateTime<Utc>>>>,
}

impl CorgigramApp {
    pub fn open_default() -> Result<Self> {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("corgigram");
        Self::open(data_dir)
    }

    pub fn open(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let config_path = data_dir.join("config.json");
        let config = AppConfig::load(&config_path);
        let storage = Storage::open(&data_dir.join("corgigram.db"))?;
        let mut app = Self {
            data_dir,
            config,
            storage,
            identity: None,
            active: Arc::new(AsyncMutex::new(None)),
            live_contact_id: Arc::new(RwLock::new(None)),
            wanted_contact_id: Arc::new(RwLock::new(None)),
            pending_offer: Arc::new(AsyncMutex::new(None)),
            pending_answer: Arc::new(AsyncMutex::new(None)),
            pending_invitations: Arc::new(RwLock::new(Vec::new())),
            contact_presence: Arc::new(RwLock::new(HashMap::new())),
            last_presence_heartbeat: Arc::new(AsyncMutex::new(None)),
            last_presence_sync: Arc::new(AsyncMutex::new(None)),
            firebase_client: Arc::new(Mutex::new(None)),
            connect_backoff: Arc::new(AsyncMutex::new(HashMap::new())),
        };
        app.load_identity()?;
        app.reconcile_stale_outbox()?;
        Ok(app)
    }

    /// Legacy outbox entries from before mailbox ack fix — treat as sent.
    fn reconcile_stale_outbox(&self) -> Result<()> {
        for item in self.storage.list_outbox(None)? {
            if item.status == "queued_firebase" {
                self.storage.update_message_status(&item.id, "sent")?;
                self.storage.delete_outbox(&item.id)?;
            }
        }
        Ok(())
    }

    async fn set_active_chat(&self, chat: ActiveChat) {
        let contact_id = chat.contact_id.clone();
        if let Ok(mut id) = self.live_contact_id.write() {
            *id = Some(contact_id.clone());
        }
        *self.active.lock().await = Some(chat);
    }

    fn connecting_contact_id(&self) -> Option<String> {
        if self
            .live_contact_id
            .read()
            .ok()
            .and_then(|g| g.clone())
            .is_some()
        {
            return None;
        }
        if let Ok(pending) = self.pending_offer.try_lock() {
            if let Some(p) = pending.as_ref() {
                return Some(p.contact_id.clone());
            }
        }
        if let Ok(pending) = self.pending_answer.try_lock() {
            if let Some(p) = pending.as_ref() {
                return Some(p.contact_id.clone());
            }
        }
        self.wanted_contact_id
            .read()
            .ok()
            .and_then(|g| g.clone())
    }

    /// UI: open chat — background poll will start/complete WebRTC for this contact.
    pub async fn set_wanted_contact(&self, contact_id: Option<String>) {
        if let Some(id) = contact_id.as_ref() {
            self.connect_backoff.lock().await.remove(id);
        }
        if let Ok(mut wanted) = self.wanted_contact_id.write() {
            *wanted = contact_id.clone();
        }
        let Some(wanted_id) = contact_id else {
            return;
        };
        if self
            .live_contact_id
            .read()
            .ok()
            .and_then(|g| g.clone())
            .as_deref()
            == Some(wanted_id.as_str())
        {
            return;
        }
        {
            let current = self
                .live_contact_id
                .read()
                .ok()
                .and_then(|g| g.clone());
            if current.as_deref() != Some(wanted_id.as_str()) && current.is_some() {
                self.disconnect_active().await;
                *self.pending_offer.lock().await = None;
            }
        }
        {
            let mut pending = self.pending_offer.lock().await;
            if let Some(p) = pending.as_ref() {
                if p.contact_id != wanted_id {
                    *pending = None;
                }
            }
        }
        let _ = self.ensure_connect_started(&wanted_id).await;
        if self.config.firebase_configured() {
            if let (Ok(me), Ok(fb)) = (self.my_user_id(), self.firebase()) {
                fb.publish_connect_ping(&wanted_id, &me).await.ok();
            }
            let _ = self.poll_signaling().await;
            let _ = self.exchange_pending_ice().await;
        }
    }

    pub async fn prefetch_turn(&self) {
        let _ = self.build_ice_config().await;
    }

    async fn disconnect_active(&self) {
        if let Ok(mut id) = self.live_contact_id.write() {
            *id = None;
        }
        let mut guard = self.active.lock().await;
        if let Some(chat) = guard.take() {
            chat.peer.close().await;
        }
    }

    async fn set_connect_backoff(&self, contact_id: &str, secs: i64) {
        let mut map = self.connect_backoff.lock().await;
        map.insert(
            contact_id.to_string(),
            Utc::now() + chrono::Duration::seconds(secs),
        );
    }

    async fn is_in_connect_backoff(&self, contact_id: &str) -> bool {
        let map = self.connect_backoff.lock().await;
        map.get(contact_id)
            .map(|until| Utc::now() < *until)
            .unwrap_or(false)
    }

    /// App startup: mark online and ping contacts to nudge WebRTC connect.
    pub async fn announce_online(&self) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let me = self.my_user_id()?;
        let fb = self.firebase()?;
        fb.publish_presence(&me, true).await?;
        for contact in self.storage.list_contacts()? {
            fb.publish_connect_ping(&contact.user_id, &me).await.ok();
        }
        Ok(())
    }

    /// Refresh own presence timestamp (called periodically while app runs).
    pub async fn heartbeat_presence(&self) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let me = self.my_user_id()?;
        self.firebase()?.publish_presence(&me, true).await
    }

    async fn maybe_heartbeat_presence(&self) {
        let mut last = self.last_presence_heartbeat.lock().await;
        let now = Utc::now();
        let due = last
            .map(|t| (now - t).num_seconds() >= 25)
            .unwrap_or(true);
        if due {
            let _ = self.heartbeat_presence().await;
            *last = Some(now);
        }
    }

    /// App shutdown: mark offline and tear down live sessions.
    pub async fn go_offline(&self) -> Result<()> {
        if self.config.firebase_configured() {
            if let Ok(me) = self.my_user_id() {
                let fb = self.firebase()?;
                fb.publish_presence(&me, false).await.ok();
                fb.clear_signaling(&me).await.ok();
            }
        }
        self.disconnect_active().await;
        if let Some(mut offer) = self.pending_offer.lock().await.take() {
            offer.peer.close().await;
        }
        if let Some(mut answer) = self.pending_answer.lock().await.take() {
            answer.peer.close().await;
        }
        Ok(())
    }

    async fn handle_connect_pings(&self) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let me = self.my_user_id()?;
        let fb = self.firebase()?;
        let pings = fb.list_connect_pings(&me).await?;
        for (from_id, _ping) in pings {
            if self.storage.get_contact(&from_id)?.is_none() {
                fb.delete_connect_ping(&me, &from_id).await.ok();
                continue;
            }
            let wanted = self
                .wanted_contact_id
                .read()
                .ok()
                .and_then(|g| g.clone());
            if wanted.as_deref() != Some(from_id.as_str()) {
                fb.delete_connect_ping(&me, &from_id).await.ok();
                continue;
            }
            if should_initiate_offer(&me, &from_id) {
                let _ = self.ensure_connect_started(&from_id).await;
            } else {
                let _ = self.poll_signaling().await;
            }
            fb.delete_connect_ping(&me, &from_id).await.ok();
        }
        Ok(())
    }

    async fn maybe_sync_contact_presence(&self) {
        let mut last = self.last_presence_sync.lock().await;
        let now = Utc::now();
        let due = last
            .map(|t| (now - t).num_seconds() >= 3)
            .unwrap_or(true);
        if due {
            let _ = self.sync_contact_presence().await;
            *last = Some(now);
        }
    }

    async fn sync_contact_presence(&self) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let fb = self.firebase()?;
        let mut cache = HashMap::new();
        for contact in self.storage.list_contacts()? {
            let online = match fb.fetch_presence(&contact.user_id).await? {
                Some(entry) => entry.is_online(),
                None => false,
            };
            cache.insert(contact.user_id.clone(), online);
        }
        if let Ok(mut stored) = self.contact_presence.write() {
            *stored = cache.clone();
        }
        Ok(())
    }

    pub fn update_config(&mut self, config: AppConfig) -> Result<()> {
        let config = config.with_normalized_firebase_url();
        config.save(&self.data_dir.join("config.json"))?;
        self.config = config;
        if let Ok(mut fb) = self.firebase_client.lock() {
            *fb = None;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<AppSnapshot> {
        Ok(AppSnapshot {
            has_identity: self.identity.is_some(),
            profile: self.profile_info(),
            contacts: self.storage.list_contacts()?,
            pending_invitations: self
                .pending_invitations
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            connected_contact_id: self
                .live_contact_id
                .read()
                .ok()
                .and_then(|g| g.clone()),
            connecting_contact_id: self.connecting_contact_id(),
            wanted_contact_id: self
                .wanted_contact_id
                .read()
                .ok()
                .and_then(|g| g.clone()),
            firebase_configured: self.config.firebase_configured(),
            firebase_database_url: self.config.effective_firebase_database_url(),
            firebase_database_url_override: self.config.firebase_database_url_override(),
            firebase_uses_default_url: self.config.uses_default_firebase_url(),
            outbox_count: self.storage.outbox_count()?,
            contact_presence: self
                .contact_presence
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
        })
    }

    pub fn create_identity(&mut self, user_id: &str, display_name: &str) -> Result<ProfileInfo> {
        let user_id = normalize_user_id(user_id)?;
        let identity = Identity::generate(&user_id, display_name);
        self.save_identity(&identity)?;
        self.identity = Some(identity);
        self.profile_info().context("profile missing")
    }

    pub fn update_profile(
        &mut self,
        display_name: Option<&str>,
        avatar_data_url: Option<&str>,
        remove_avatar: bool,
    ) -> Result<ProfileInfo> {
        if self.identity.is_none() {
            anyhow::bail!("no identity");
        }
        if let Some(name) = display_name {
            let name = name.trim();
            if name.is_empty() {
                anyhow::bail!("nickname cannot be empty");
            }
            if name.chars().count() > 64 {
                anyhow::bail!("nickname too long (max 64)");
            }
            {
                let identity = self.identity.as_mut().unwrap();
                identity.public.display_name = name.to_string();
            }
            let identity = self.identity.as_ref().unwrap();
            self.save_identity(identity)?;
        }
        if remove_avatar {
            self.clear_avatar()?;
        } else if let Some(data_url) = avatar_data_url {
            if !data_url.is_empty() {
                self.save_avatar(data_url)?;
            }
        }
        self.profile_info().context("profile missing")
    }

    pub fn profile_info(&self) -> Option<ProfileInfo> {
        self.identity.as_ref().map(|id| ProfileInfo {
            user_id: id.public.user_id.clone(),
            display_name: id.public.display_name.clone(),
            bundle_json: serde_json::to_string_pretty(&id.prekey_bundle()).unwrap_or_default(),
            safety_hint: "Share bundle via QR — verify safety number in chat".into(),
            avatar_data_url: self.load_avatar_data_url(),
        })
    }

    pub fn bundle_qr_png_base64(&self) -> Result<String> {
        let identity = self.identity.as_ref().context("no identity")?;
        let bundle_json = serde_json::to_string(&identity.prekey_bundle())?;
        let code = QrCode::new(bundle_json.as_bytes()).context("qr encode")?;
        let image = code.render::<qrcode::render::svg::Color>().build();
        let b64 = base64::engine::general_purpose::STANDARD.encode(image.as_bytes());
        Ok(format!("data:image/svg+xml;base64,{b64}"))
    }

    pub fn add_contact_from_bundle_json(&mut self, bundle_json: &str) -> Result<ContactRecord> {
        let bundle: PreKeyBundle = serde_json::from_str(bundle_json)?;
        bundle.verify().map_err(|e| anyhow::anyhow!("invalid bundle: {e}"))?;
        self.add_contact_from_bundle(bundle)
    }

    pub fn add_contact_from_bundle(&mut self, bundle: PreKeyBundle) -> Result<ContactRecord> {
        bundle.verify().map_err(|e| anyhow::anyhow!("invalid bundle: {e}"))?;
        if self
            .my_user_id()
            .ok()
            .is_some_and(|me| me == bundle.identity.user_id)
        {
            anyhow::bail!("cannot add yourself as a contact");
        }
        let contact = ContactRecord {
            user_id: bundle.identity.user_id.clone(),
            display_name: bundle.identity.display_name.clone(),
            bundle,
            created_at: Utc::now(),
            avatar_data_url: None,
        };
        self.storage.upsert_contact(&contact)?;
        Ok(contact)
    }

    pub async fn request_contact_avatar(&self, owner_id: &str) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let me = self.my_user_id()?;
        if me == owner_id {
            return Ok(());
        }
        self.firebase()?.request_avatar(owner_id, &me).await
    }

    /// Publish directory bundle + E2E encrypted avatar copies for contacts.
    pub async fn sync_directory(&self) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let identity = self.identity.as_ref().context("no identity")?;
        let bundle = identity.prekey_bundle();
        let me = identity.public.user_id.clone();
        let fb = self.firebase()?;
        fb.publish_directory(&me, &bundle).await?;
        self.sync_avatar_uploads().await
    }

    /// Fetch encrypted contact avatars from Firebase and refresh local cache.
    pub async fn sync_avatars(&self) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        self.sync_avatar_uploads().await?;
        self.sync_avatar_downloads().await
    }

    async fn sync_avatar_uploads(&self) -> Result<()> {
        let identity = self.identity.as_ref().context("no identity")?;
        let me = identity.public.user_id.clone();
        let fb = self.firebase()?;

        let mut viewers: HashSet<String> = self
            .storage
            .list_contacts()?
            .into_iter()
            .map(|c| c.user_id)
            .collect();
        for viewer_id in fb.list_avatar_wants(&me).await? {
            viewers.insert(viewer_id);
        }

        let avatar_bytes = self.load_avatar_bytes()?;
        let mime = self
            .storage
            .get_meta("avatar_mime")
            .ok()
            .flatten()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "image/png".into());

        for viewer_id in viewers {
            if viewer_id == me {
                continue;
            }
            let viewer_bundle = match self.storage.get_contact(&viewer_id)? {
                Some(c) => c.bundle,
                None => match fb.fetch_directory(&viewer_id).await? {
                    Some(b) => b,
                    None => continue,
                },
            };
            if let Some(bytes) = &avatar_bytes {
                let ciphertext = encrypt_avatar(identity, &viewer_bundle, bytes)
                    .map_err(|e| anyhow::anyhow!("avatar encrypt: {e}"))?;
                fb.publish_avatar(&me, &viewer_id, &ciphertext, &mime).await?;
            } else {
                fb.delete_avatar(&me, &viewer_id).await.ok();
            }
        }
        Ok(())
    }

    pub async fn sync_avatar_downloads(&self) -> Result<()> {
        let identity = self.identity.as_ref().context("no identity")?;
        let me = identity.public.user_id.clone();
        let fb = self.firebase()?;

        for contact in self.storage.list_contacts()? {
            let Some(entry) = fb.fetch_avatar(&contact.user_id, &me).await? else {
                continue;
            };
            let blob = base64::engine::general_purpose::STANDARD
                .decode(&entry.ciphertext)
                .context("avatar ciphertext base64")?;
            let plain = decrypt_avatar(identity, &contact.bundle, &blob)
                .map_err(|e| anyhow::anyhow!("avatar decrypt from {}: {e}", contact.user_id))?;
            self.save_contact_avatar(&contact.user_id, &plain, &entry.mime)?;
        }
        Ok(())
    }

    pub async fn add_contact_by_user_id(&mut self, user_id: &str) -> Result<ContactRecord> {
        if !self.config.firebase_configured() {
            anyhow::bail!("lookup by ID requires Firebase (enabled by default)");
        }
        let user_id = normalize_user_id(user_id)?;
        if self.my_user_id()? == user_id {
            anyhow::bail!("cannot add yourself as a contact");
        }
        if self.storage.get_contact(&user_id)?.is_some() {
            anyhow::bail!("contact already exists");
        }
        let fb = self.firebase()?;
        let bundle = fb
            .fetch_directory(&user_id)
            .await?
            .with_context(|| format!("user '{user_id}' not found — they must open Corgigram at least once"))?;
        let contact = self.add_contact_from_bundle(bundle)?;
        let owner_id = contact.user_id.clone();
        let me = self.my_user_id()?;
        let identity = self.identity.as_ref().context("no identity")?;
        let my_name = identity.public.display_name.clone();
        let my_bundle = identity.prekey_bundle();
        let _ = self.sync_directory().await;
        fb.publish_invitation(&owner_id, &me, &my_name, &my_bundle)
            .await
            .ok();
        self.request_contact_avatar(&owner_id).await?;
        self.sync_avatar_downloads().await?;
        self.contacts_with_avatars()?
            .into_iter()
            .find(|c| c.user_id == owner_id)
            .context("contact missing after add")
    }

    pub fn safety_number(&self, contact_id: &str) -> Result<String> {
        let identity = self.identity.as_ref().context("no identity")?;
        let contact = self
            .storage
            .get_contact(contact_id)?
            .context("contact not found")?;
        Ok(identity.public.safety_number(&contact.bundle.identity))
    }

    pub async fn sync_invitations(&self) -> Result<Vec<InvitationInfo>> {
        if !self.config.firebase_configured() {
            return Ok(vec![]);
        }
        let me = self.my_user_id()?;
        let fb = self.firebase()?;
        let entries = fb.list_invitations(&me).await?;
        let mut invitations = Vec::new();
        for (from_id, entry) in entries {
            if self.storage.get_contact(&from_id)?.is_some() {
                fb.delete_invitation(&me, &from_id).await.ok();
                continue;
            }
            invitations.push(InvitationInfo {
                from_user_id: from_id,
                display_name: entry.display_name,
            });
        }
        if let Ok(mut cache) = self.pending_invitations.write() {
            *cache = invitations.clone();
        }
        Ok(invitations)
    }

    pub async fn accept_invitation(&mut self, from_user_id: &str) -> Result<ContactRecord> {
        if !self.config.firebase_configured() {
            anyhow::bail!("invitations require Firebase");
        }
        let from_user_id = normalize_user_id(from_user_id)?;
        let me = self.my_user_id()?;
        if from_user_id == me {
            anyhow::bail!("invalid invitation");
        }
        if self.storage.get_contact(&from_user_id)?.is_some() {
            self.firebase()?
                .delete_invitation(&me, &from_user_id)
                .await
                .ok();
            anyhow::bail!("contact already exists");
        }
        let fb = self.firebase()?;
        let invitation = fb.fetch_invitation(&me, &from_user_id).await?;
        let bundle = if let Some(inv) = invitation.as_ref().and_then(|i| i.bundle.clone()) {
            inv
        } else {
            let mut fetched = None;
            for attempt in 0..5 {
                if let Some(bundle) = fb.fetch_directory(&from_user_id).await? {
                    fetched = Some(bundle);
                    break;
                }
                if attempt < 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                }
            }
            fetched.with_context(|| {
                format!(
                    "пользователь «{from_user_id}» не найден — попросите его открыть Corgigram и нажать «Принять» снова"
                )
            })?
        };
        let _contact = self.add_contact_from_bundle(bundle)?;
        fb.delete_invitation(&me, &from_user_id).await.ok();
        self.request_contact_avatar(&from_user_id).await?;
        self.sync_avatar_downloads().await?;
        let _ = self.sync_invitations().await;
        self.contacts_with_avatars()?
            .into_iter()
            .find(|c| c.user_id == from_user_id)
            .context("contact missing after accept")
    }

    pub async fn decline_invitation(&self, from_user_id: &str) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let from_user_id = normalize_user_id(from_user_id)?;
        let me = self.my_user_id()?;
        self.firebase()?
            .delete_invitation(&me, &from_user_id)
            .await?;
        let _ = self.sync_invitations().await;
        Ok(())
    }

    pub fn messages(&self, contact_id: &str) -> Result<Vec<MessageRecord>> {
        Ok(self.storage.list_messages(contact_id)?)
    }

    pub fn messages_page(
        &self,
        contact_id: &str,
        before_created_at: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MessageRecord>> {
        Ok(self
            .storage
            .list_messages_page(contact_id, before_created_at, limit)?)
    }

    pub fn get_contact_avatar(&self, contact_id: &str) -> Option<String> {
        self.load_contact_avatar_data_url(contact_id)
    }

    pub fn read_attachment(&self, message_id: &str, index: usize) -> Result<AttachmentData> {
        chat_media::read_attachment_bytes(&self.data_dir, message_id, index)
    }

    pub fn attachment_count(&self, message_id: &str) -> usize {
        chat_media::attachment_count(&self.data_dir, message_id)
    }

    fn contacts_fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Ok(contacts) = self.storage.list_contacts() {
            for contact in contacts {
                contact.user_id.hash(&mut hasher);
            }
        }
        if let Ok(presence) = self.contact_presence.read() {
            for (user_id, online) in presence.iter() {
                user_id.hash(&mut hasher);
                online.hash(&mut hasher);
            }
        }
        if let Ok(live) = self.live_contact_id.read() {
            live.hash(&mut hasher);
        }
        self.connecting_contact_id().hash(&mut hasher);
        if let Ok(count) = self.storage.outbox_count() {
            count.hash(&mut hasher);
        }
        if let Ok(invitations) = self.pending_invitations.read() {
            invitations.len().hash(&mut hasher);
            for invitation in invitations.iter() {
                invitation.from_user_id.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    pub async fn background_tick(&self) -> Result<BackgroundTickResult> {
        let fingerprint_before = self.contacts_fingerprint();
        let connecting = self.connecting_contact_id().is_some();
        let mut messages = self.sync_all_mailboxes().await.unwrap_or_default();
        let mut status_updates = self.sync_delivery_acks().await.unwrap_or_default();
        status_updates.extend(self.sync_mailbox_consumed().await.unwrap_or_default());
        let _ = self.sync_invitations().await;
        let _ = self.poll_connectivity().await;
        messages.extend(self.recv_live_messages().await.unwrap_or_default());
        let _ = self.exchange_pending_ice().await;
        if connecting {
            let _ = self.exchange_pending_ice().await;
        }
        let fingerprint_after = self.contacts_fingerprint();
        Ok(BackgroundTickResult {
            contacts_changed: fingerprint_before != fingerprint_after
                || !messages.is_empty()
                || !status_updates.is_empty(),
            messages,
            status_updates,
            connecting,
        })
    }

    async fn sync_delivery_acks(&self) -> Result<Vec<(String, String)>> {
        if !self.config.firebase_configured() {
            return Ok(vec![]);
        }
        let me = self.my_user_id()?;
        let fb = self.firebase()?;
        let acks = fb.list_delivery_acks(&me).await?;
        let mut updated = Vec::new();
        for (msg_id, _ack) in acks {
            if self.storage.mark_delivered_if_queued(&msg_id)? {
                updated.push((msg_id.clone(), "delivered".to_string()));
            }
            fb.delete_delivery_ack(&me, &msg_id).await.ok();
        }
        Ok(updated)
    }

    /// Sender-side: if our mailbox entry was consumed by the recipient, mark delivered.
    async fn sync_mailbox_consumed(&self) -> Result<Vec<(String, String)>> {
        if !self.config.firebase_configured() {
            return Ok(vec![]);
        }
        let fb = self.firebase()?;
        let queued = self.storage.list_outbound_by_status("queued_mailbox")?;
        let mut updated = Vec::new();
        let now = Utc::now();
        for msg in queued {
            if now.signed_duration_since(msg.created_at) < chrono::Duration::seconds(15) {
                continue;
            }
            if fb
                .mailbox_entry_exists(&msg.contact_id, &msg.id)
                .await
                .unwrap_or(true)
            {
                continue;
            }
            if self.storage.mark_delivered_if_queued(&msg.id)? {
                updated.push((msg.id.clone(), "delivered".to_string()));
            }
        }
        Ok(updated)
    }

    async fn build_ice_config(&self) -> IceConfig {
        if !self.config.firebase_configured() {
            return IceConfig::localhost();
        }
        let mut ice = IceConfig::default();
        if let Ok(me) = self.my_user_id() {
            if let Ok(turn) = fetch_elixir_webrtc_turn(&me).await {
                ice.add_turn_server(turn);
            }
        }
        ice
    }

    /// Exchange trickle ICE for in-progress offer/answer handshakes.
    pub async fn exchange_pending_ice(&self) -> Result<()> {
        let me = self.my_user_id()?;
        if let Some(offer) = self.pending_offer.lock().await.as_mut() {
            let contact_id = offer.contact_id.clone();
            self.exchange_ice(&offer.peer, &me, &contact_id, &mut offer.seen_ice)
                .await?;
        }
        if let Some(answer) = self.pending_answer.lock().await.as_mut() {
            let contact_id = answer.contact_id.clone();
            self.exchange_ice(&answer.peer, &me, &contact_id, &mut answer.seen_ice)
                .await?;
        }
        Ok(())
    }

    fn firebase(&self) -> Result<FirebaseSignaling> {
        if !self.config.firebase_configured() {
            anyhow::bail!("firebase not configured");
        }
        let mut guard = self
            .firebase_client
            .lock()
            .map_err(|_| anyhow::anyhow!("firebase client lock poisoned"))?;
        if guard.is_none() {
            *guard = Some(FirebaseSignaling::new(
                &self.config.effective_firebase_database_url(),
                self.config.firebase_auth_token.clone(),
            ));
        }
        guard.clone().context("firebase client missing")
    }

    fn my_user_id(&self) -> Result<String> {
        Ok(self
            .identity
            .as_ref()
            .context("no identity")?
            .public
            .user_id
            .clone())
    }

    pub async fn diagnose_connect(&self, contact_id: &str) -> Result<ConnectDiagnose> {
        let me = self.my_user_id()?;
        let fb = self.firebase()?;
        let has_firebase_offer_to_me = fb.fetch_offer(&me).await?.is_some();
        let has_firebase_answer_from_contact = fb.fetch_answer(&me, contact_id).await?.is_some();
        let local_ice_to_contact = fb.list_ice_candidates(contact_id, &me).await.map(|v| v.len()).unwrap_or(0);
        let remote_ice_from_contact = fb.list_ice_candidates(&me, contact_id).await.map(|v| v.len()).unwrap_or(0);
        let turn_fetched = fetch_elixir_webrtc_turn(&me).await.is_ok();
        Ok(ConnectDiagnose {
            my_user_id: me,
            contact_id: contact_id.to_string(),
            has_pending_offer: self.pending_offer.lock().await.is_some(),
            has_pending_answer: self.pending_answer.lock().await.is_some(),
            is_active: self.live_contact_id.read().ok().and_then(|g| g.clone()).is_some(),
            has_firebase_offer_to_me,
            has_firebase_answer_from_contact,
            local_ice_to_contact,
            remote_ice_from_contact,
            turn_fetched,
        })
    }

    /// Offerer: create offer, optionally wait for answer via Firebase, complete handshake.
    pub async fn connect_auto(&self, contact_id: &str) -> Result<ConnectAutoResult> {
        let offer = self.connect_offer(contact_id).await?;
        if offer.auto_signaling {
            let me = self.my_user_id()?;
            let fb = self.firebase()?;
            let answer = self.wait_answer_with_ice(&me, contact_id, 90).await?;
            self.connect_finish(contact_id, &answer).await?;
            fb.clear_signaling(&me).await.ok();
            fb.clear_signaling(contact_id).await.ok();
            self.sync_mailbox(contact_id).await?;
            self.flush_outbox(contact_id).await?;
            return Ok(ConnectAutoResult {
                contact_id: contact_id.to_string(),
                connected: true,
            });
        }
        Ok(ConnectAutoResult {
            contact_id: contact_id.to_string(),
            connected: false,
        })
    }

    async fn wait_answer_with_ice(&self, me: &str, contact_id: &str, timeout_secs: u64) -> Result<String> {
        let fb = self.firebase()?;
        for _ in 0..(timeout_secs * 5) {
            {
                let mut pending = self.pending_offer.lock().await;
                if let Some(p) = pending.as_mut() {
                    if p.contact_id == contact_id {
                        let _ = self
                            .exchange_ice(&p.peer, me, contact_id, &mut p.seen_ice)
                            .await;
                    }
                }
            }
            if let Some(answer) = fb.fetch_answer(me, contact_id).await? {
                return Ok(answer.sdp);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        anyhow::bail!("timed out waiting for firebase answer")
    }

    async fn ensure_connect_started(&self, contact_id: &str) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let me = self.my_user_id()?;
        // Glare avoidance: lexicographically lower user_id initiates the offer.
        if !should_initiate_offer(&me, contact_id) {
            return Ok(());
        }
        if self
            .live_contact_id
            .read()
            .ok()
            .and_then(|g| g.clone())
            .as_deref()
            == Some(contact_id)
        {
            return Ok(());
        }
        if self.pending_answer.lock().await.is_some() {
            return Ok(());
        }
        {
            let mut pending = self.pending_offer.lock().await;
            if let Some(p) = pending.as_ref() {
                if p.contact_id == contact_id {
                    return Ok(());
                }
                if let Some(mut old) = pending.take() {
                    old.peer.close().await;
                }
            }
        }
        {
            let active = self.active.lock().await;
            if let Some(chat) = active.as_ref() {
                if chat.contact_id == contact_id {
                    return Ok(());
                }
            }
        }
        self.connect_offer(contact_id).await?;
        Ok(())
    }

    async fn advance_pending_offer(&self) -> Result<Option<String>> {
        let contact_id = {
            let pending = self.pending_offer.lock().await;
            let Some(offer) = pending.as_ref() else {
                return Ok(None);
            };
            offer.contact_id.clone()
        };

        let me = self.my_user_id()?;
        for _ in 0..30 {
            {
                let mut pending = self.pending_offer.lock().await;
                let Some(ref mut offer) = pending.as_mut() else {
                    return Ok(None);
                };
                if offer.contact_id != contact_id {
                    return Ok(None);
                }
                self.exchange_ice(&offer.peer, &me, &contact_id, &mut offer.seen_ice)
                    .await?;
            }
            if let Some(answer) = self.firebase()?.fetch_answer(&me, &contact_id).await? {
                self.connect_finish(&contact_id, &answer.sdp).await?;
                let fb = self.firebase()?;
                fb.clear_signaling(&me).await.ok();
                fb.clear_signaling(&contact_id).await.ok();
                return Ok(Some(contact_id));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Ok(None)
    }

    async fn maintain_wanted_connection(&self) -> Result<()> {
        let wanted = self
            .wanted_contact_id
            .read()
            .ok()
            .and_then(|g| g.clone());
        let Some(contact_id) = wanted else {
            return Ok(());
        };
        if self.is_in_connect_backoff(&contact_id).await {
            return Ok(());
        }
        if self
            .live_contact_id
            .read()
            .ok()
            .and_then(|g| g.clone())
            .as_deref()
            == Some(contact_id.as_str())
        {
            return Ok(());
        }
        let _ = self.ensure_connect_started(&contact_id).await;
        Ok(())
    }

    pub async fn connect_offer(&self, contact_id: &str) -> Result<ConnectOfferResult> {
        self.storage.get_contact(contact_id)?.context("contact not found")?;

        if self.config.firebase_configured() {
            let me = self.my_user_id()?;
            let fb = self.firebase()?;
            fb.clear_signaling(&me).await.ok();
        }

        let (peer, offer_sdp) = run_offerer_role(&self.build_ice_config().await).await?;

        if self.config.firebase_configured() {
            let me = self.my_user_id()?;
            self.firebase()?
                .publish_offer(contact_id, &me, &offer_sdp)
                .await?;
        }

        *self.pending_offer.lock().await = Some(PendingOffer {
            contact_id: contact_id.to_string(),
            peer,
            seen_ice: HashSet::new(),
            started_at: Utc::now(),
        });

        Ok(ConnectOfferResult {
            offer_sdp,
            contact_id: contact_id.to_string(),
            auto_signaling: self.config.firebase_configured(),
        })
    }

    pub async fn connect_finish(&self, contact_id: &str, answer_sdp: &str) -> Result<()> {
        self.finish_pending_offer(contact_id, answer_sdp).await?;
        self.sync_mailbox(contact_id).await?;
        self.flush_outbox(contact_id).await?;
        Ok(())
    }

    async fn finish_pending_offer(&self, contact_id: &str, answer_sdp: &str) -> Result<()> {
        let identity = self.identity.as_ref().context("no identity")?.clone();
        let contact = self
            .storage
            .get_contact(contact_id)?
            .context("contact not found")?;

        {
            let mut pending = self.pending_offer.lock().await;
            let Some(ref mut pending_offer) = pending.as_mut() else {
                anyhow::bail!("no pending offer");
            };
            if pending_offer.contact_id != contact_id {
                anyhow::bail!("pending offer is for another contact");
            }
            pending_offer.peer.apply_remote_answer(answer_sdp).await?;
        }

        // Keep pending_offer registered so background ICE polling can run in parallel.
        self.connect_with_ice_via_pending(contact_id).await?;

        let mut pending = self.pending_offer.lock().await;
        let Some(mut pending_offer) = pending.take() else {
            anyhow::bail!("no pending offer");
        };

        let session = self
            .run_session_handshake_as_initiator(&mut pending_offer.peer, identity, &contact.bundle)
            .await?;

        self.set_active_chat(ActiveChat {
            contact_id: contact_id.to_string(),
            peer: pending_offer.peer,
            session,
        })
        .await;
        Ok(())
    }

    async fn connect_with_ice_via_pending(&self, contact_id: &str) -> Result<()> {
        let me = self.my_user_id()?;
        for _ in 0..400 {
            {
                let mut pending = self.pending_offer.lock().await;
                let Some(ref mut pending_offer) = pending.as_mut() else {
                    anyhow::bail!("no pending offer");
                };
                if pending_offer.contact_id != contact_id {
                    anyhow::bail!("pending offer is for another contact");
                }
                for _ in 0..5 {
                    self.exchange_ice(
                        &pending_offer.peer,
                        &me,
                        contact_id,
                        &mut pending_offer.seen_ice,
                    )
                    .await?;
                }
                if pending_offer.peer.is_connected() {
                    return Ok(());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        anyhow::bail!("timed out waiting for peer connection")
    }

    pub async fn connect_answer(&self, offer_sdp: &str, contact_id: &str) -> Result<ConnectAnswerResult> {
        let identity = self.identity.as_ref().context("no identity")?.clone();
        let contact = self
            .storage
            .get_contact(contact_id)?
            .context("contact not found")?;

        let (mut peer, answer_sdp) = run_answerer_role(&self.build_ice_config().await, offer_sdp).await?;
        let mut seen_ice = HashSet::new();
        let me = self.my_user_id()?;

        if self.config.firebase_configured() {
            self.firebase()?
                .publish_answer(contact_id, &me, &answer_sdp)
                .await?;
        }

        self.connect_with_ice(&peer, &me, contact_id, &mut seen_ice).await?;
        peer.wait_ready().await?;
        let session = self
            .run_session_handshake_as_responder(&mut peer, identity, &contact.bundle)
            .await?;

        self.set_active_chat(ActiveChat {
            contact_id: contact_id.to_string(),
            peer,
            session,
        })
        .await;

        Ok(ConnectAnswerResult {
            answer_sdp,
            contact_id: contact_id.to_string(),
        })
    }

    async fn begin_connect_answer(&self, offer_sdp: &str, contact_id: &str) -> Result<()> {
        if self.pending_answer.lock().await.is_some() {
            return Ok(());
        }
        let (peer, answer_sdp) = run_answerer_role(&self.build_ice_config().await, offer_sdp).await?;
        if self.config.firebase_configured() {
            let me = self.my_user_id()?;
            self.firebase()?
                .publish_answer(contact_id, &me, &answer_sdp)
                .await?;
        }
        *self.pending_answer.lock().await = Some(PendingAnswer {
            contact_id: contact_id.to_string(),
            peer,
            seen_ice: HashSet::new(),
            started_at: Utc::now(),
        });
        Ok(())
    }

    async fn advance_pending_answer(&self) -> Result<Option<String>> {
        let contact_id = {
            let pending = self.pending_answer.lock().await;
            let Some(answer) = pending.as_ref() else {
                return Ok(None);
            };
            answer.contact_id.clone()
        };

        let me = self.my_user_id()?;
        for _ in 0..25 {
            let connected = {
                let mut pending = self.pending_answer.lock().await;
                let Some(ref mut answer) = pending.as_mut() else {
                    return Ok(None);
                };
                self.exchange_ice(&answer.peer, &me, &contact_id, &mut answer.seen_ice)
                    .await?;
                answer.peer.is_connected()
            };
            if connected {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let mut pending = self.pending_answer.lock().await;
        let Some(mut answer) = pending.as_mut() else {
            return Ok(None);
        };

        if !answer.peer.is_connected() {
            return Ok(None);
        }

        if answer.peer.wait_ready().await.is_err() {
            return Ok(None);
        }

        let identity = self.identity.as_ref().context("no identity")?.clone();
        let contact = self
            .storage
            .get_contact(&contact_id)?
            .context("contact not found")?;
        let session = match self
            .run_session_handshake_as_responder(&mut answer.peer, identity, &contact.bundle)
            .await
        {
            Ok(session) => session,
            Err(_) => return Ok(None),
        };

        let answer = pending.take().expect("pending answer");
        drop(pending);

        self.set_active_chat(ActiveChat {
            contact_id: contact_id.clone(),
            peer: answer.peer,
            session,
        })
        .await;
        self.firebase()?.clear_signaling(&me).await.ok();
        Ok(Some(contact_id))
    }

    async fn exchange_ice(
        &self,
        peer: &PeerConnection,
        me: &str,
        peer_id: &str,
        seen: &mut HashSet<String>,
    ) -> Result<()> {
        if !self.config.firebase_configured() {
            return Ok(());
        }
        let fb = self.firebase()?;
        for candidate in peer.drain_local_candidates().await {
            let id = uuid::Uuid::new_v4().to_string();
            let _ = fb.publish_ice_candidate(peer_id, me, &id, &candidate).await;
        }
        if seen.insert("__eoc_sent__".to_string()) {
            let eoc = serde_json::json!({"candidate":""}).to_string();
            let _ = fb
                .publish_ice_candidate(peer_id, me, "__eoc__", &eoc)
                .await;
        }
        if let Ok(candidates) = fb.list_ice_candidates(me, peer_id).await {
            for (id, candidate) in candidates {
                if id == "__eoc__" {
                    if seen.insert(id) {
                        let eoc = serde_json::json!({"candidate":""}).to_string();
                        peer.add_remote_candidate(&eoc).await.ok();
                    }
                    continue;
                }
                if seen.insert(id) {
                    peer.add_remote_candidate(&candidate).await.ok();
                }
            }
        }
        Ok(())
    }

    async fn connect_with_ice(
        &self,
        peer: &PeerConnection,
        me: &str,
        peer_id: &str,
        seen: &mut HashSet<String>,
    ) -> Result<()> {
        for _ in 0..400 {
            for _ in 0..5 {
                let _ = self.exchange_ice(peer, me, peer_id, seen).await;
            }
            if peer.is_connected() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        anyhow::bail!("timed out waiting for peer connection")
    }

    async fn recover_stale_handshakes(&self) {
        let offer_stale = chrono::Duration::seconds(55);
        let answer_stale = chrono::Duration::seconds(40);
        let now = Utc::now();

        {
            let mut pending = self.pending_answer.lock().await;
            if let Some(answer) = pending.as_ref() {
                if now.signed_duration_since(answer.started_at) > answer_stale
                    && !answer.peer.is_connected()
                {
                    let contact_id = answer.contact_id.clone();
                    if let Some(mut answer) = pending.take() {
                        answer.peer.close().await;
                        drop(pending);
                        self.set_connect_backoff(&contact_id, 2).await;
                    }
                }
            }
        }
        {
            let mut pending = self.pending_offer.lock().await;
            if let Some(offer) = pending.as_ref() {
                if now.signed_duration_since(offer.started_at) > offer_stale
                    && !offer.peer.is_connected()
                {
                    let contact_id = offer.contact_id.clone();
                    if let Some(mut offer) = pending.take() {
                        offer.peer.close().await;
                        drop(pending);
                        self.set_connect_backoff(&contact_id, 2).await;
                    }
                }
            }
        }
    }

    /// Poll Firebase for incoming WebRTC offers and auto-answer known contacts.
    pub async fn poll_signaling(&self) -> Result<Option<String>> {
        if !self.config.firebase_configured() {
            return Ok(None);
        }
        if self.active.lock().await.is_some() {
            return Ok(None);
        }
        if self.pending_answer.lock().await.is_some() {
            return Ok(None);
        }

        let me = self.my_user_id()?;
        let fb = self.firebase()?;
        let Some(offer) = fb.fetch_offer(&me).await? else {
            return Ok(None);
        };

        if self.storage.get_contact(&offer.from)?.is_none() {
            return Ok(None);
        }

        {
            let mut pending = self.pending_offer.lock().await;
            if let Some(p) = pending.as_ref() {
                if p.contact_id == offer.from {
                    if !should_initiate_offer(&me, &offer.from) {
                        *pending = None;
                    } else {
                        return Ok(None);
                    }
                }
            }
        }

        self.begin_connect_answer(&offer.sdp, &offer.from).await?;
        Ok(Some(offer.from))
    }

    pub async fn send_message(&self, contact_id: &str, text: &str) -> Result<MessageRecord> {
        let payload = ChatPayload::Text {
            body: text.to_string(),
        };
        self.send_payload(contact_id, &payload).await
    }

    pub async fn send_attachments(
        &self,
        contact_id: &str,
        attachments: Vec<OutgoingAttachment>,
        caption: Option<&str>,
    ) -> Result<MessageRecord> {
        let payload = chat_media::build_payload(caption, &attachments)?;
        self.send_payload(contact_id, &payload).await
    }

    async fn send_payload(
        &self,
        contact_id: &str,
        payload: &ChatPayload,
    ) -> Result<MessageRecord> {
        let msg_id = Storage::new_message_id();
        let plain = payload_to_bytes(payload)?;
        save_payload_attachments(&self.data_dir, &msg_id, payload)?;
        let record =
            message_record_from_payload(&msg_id, contact_id, "out", "pending", payload);

        if !prefers_mailbox_delivery(payload, plain.len()) {
            match self
                .try_send_payload_bytes(contact_id, &plain, &msg_id, &record)
                .await
            {
                Ok(Some(sent)) => return Ok(sent),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("live send failed, queueing to mailbox: {error:#}");
                }
            }
        }

        self.queue_offline_payload(contact_id, &plain, &record).await
    }

    async fn try_send_payload_bytes(
        &self,
        contact_id: &str,
        plain: &[u8],
        msg_id: &str,
        display: &MessageRecord,
    ) -> Result<Option<MessageRecord>> {
        if matches!(
            self.storage.get_message_status(msg_id)?,
            Some(status) if status == "queued_mailbox"
        ) {
            return Ok(None);
        }

        let mut guard = self.active.lock().await;
        let Some(active) = guard.as_mut() else {
            return Ok(None);
        };
        if active.contact_id != contact_id {
            return Ok(None);
        }

        let encrypted = active.session.encrypt(plain)?;
        if let Err(error) = active
            .peer
            .send(&WireMessage::EncryptedChat { ciphertext: encrypted }.to_bytes()?)
            .await
        {
            return Err(error);
        }

        let mut record = display.clone();
        record.id = msg_id.to_string();
        record.status = "sent".into();
        self.storage.upsert_message(&record)?;
        Ok(Some(record))
    }

    fn ingest_incoming_plain(
        &self,
        msg_id: &str,
        contact_id: &str,
        plain: &[u8],
    ) -> Result<MessageRecord> {
        let payload = bytes_to_payload(plain);
        save_payload_attachments(&self.data_dir, msg_id, &payload)?;
        let record =
            message_record_from_payload(msg_id, contact_id, "in", "delivered", &payload);
        self.storage.upsert_message(&record)?;
        Ok(record)
    }

    async fn queue_offline_payload(
        &self,
        contact_id: &str,
        plain: &[u8],
        record: &MessageRecord,
    ) -> Result<MessageRecord> {
        let identity = self.identity.as_ref().context("no identity")?.clone();
        let contact = self
            .storage
            .get_contact(contact_id)?
            .context("contact not found")?;
        let me = identity.public.user_id.clone();

        let ciphertext = encrypt_mailbox(&identity, &contact.bundle, plain)?;
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(plain);

        if self.config.firebase_configured() {
            let fb = self.firebase()?;
            fb.publish_mailbox(contact_id, &record.id, &me, &ciphertext)
                .await?;
            fb.publish_mailbox_ping(contact_id, &me, &record.id)
                .await
                .ok();
            let queued = MessageRecord {
                status: "queued_mailbox".into(),
                ..record.clone()
            };
            self.storage.upsert_message(&queued)?;
            return Ok(queued);
        }

        self.storage.insert_outbox(&OutboxRecord {
            id: record.id.clone(),
            contact_id: contact_id.to_string(),
            body: payload_b64,
            status: "queued_local".into(),
            created_at: record.created_at,
        })?;
        let queued = MessageRecord {
            status: "queued_local".into(),
            ..record.clone()
        };
        self.storage.upsert_message(&queued)?;
        Ok(queued)
    }

    async fn decrypt_mailbox_for_contact(
        &self,
        identity: &Identity,
        contact: &ContactRecord,
        bytes: &[u8],
        fb: &FirebaseSignaling,
    ) -> Result<Vec<u8>> {
        match decrypt_mailbox(identity, &contact.bundle, bytes) {
            Ok(plain) => Ok(plain),
            Err(_) => {
                let Some(fresh) = fb.fetch_directory(&contact.user_id).await? else {
                    anyhow::bail!("mailbox decrypt failed");
                };
                fresh.verify().map_err(|e| anyhow::anyhow!("invalid bundle: {e}"))?;
                let updated = ContactRecord {
                    bundle: fresh.clone(),
                    ..contact.clone()
                };
                self.storage.upsert_contact(&updated)?;
                decrypt_mailbox(identity, &fresh, bytes).map_err(|e| anyhow::anyhow!("mailbox decrypt: {e}"))
            }
        }
    }

    async fn process_mailbox_entry(
        &self,
        msg_id: &str,
        entry: &MailboxEntry,
        fb: &FirebaseSignaling,
        identity: &Identity,
        me: &str,
    ) -> Result<Option<MessageRecord>> {
        if self.storage.message_exists(msg_id)? {
            fb.delete_mailbox(me, msg_id).await.ok();
            self.publish_mailbox_delivery_ack(fb, &entry.from, msg_id, me)
                .await;
            return Ok(None);
        }
        let contact = match self.storage.get_contact(&entry.from)? {
            Some(contact) => contact,
            None => return Ok(None),
        };
        let bytes = match fb
            .fetch_mailbox_ciphertext(me, msg_id, entry)
            .await
        {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        let plain = match self
            .decrypt_mailbox_for_contact(identity, &contact, &bytes, fb)
            .await
        {
            Ok(plain) => plain,
            Err(_) => return Ok(None),
        };
        let record = self.ingest_incoming_plain(msg_id, &entry.from, &plain)?;
        fb.delete_mailbox(me, msg_id).await.ok();
        self.publish_mailbox_delivery_ack(fb, &entry.from, msg_id, me)
            .await;
        Ok(Some(record))
    }

    async fn publish_mailbox_delivery_ack(
        &self,
        fb: &FirebaseSignaling,
        sender_id: &str,
        msg_id: &str,
        me: &str,
    ) {
        for attempt in 0..3 {
            match fb.publish_delivery_ack(sender_id, msg_id, me).await {
                Ok(()) => return,
                Err(error) => {
                    eprintln!(
                        "delivery ack failed (attempt {}): {error:#}",
                        attempt + 1
                    );
                    if attempt < 2 {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
            }
        }
    }

    pub async fn sync_mailbox(&self, contact_id: &str) -> Result<Vec<MessageRecord>> {
        let received = self.sync_all_mailboxes().await?;
        Ok(received
            .into_iter()
            .filter(|message| message.contact_id == contact_id)
            .collect())
    }

    pub async fn flush_outbox(&self, contact_id: &str) -> Result<()> {
        let items = self.storage.list_outbox(Some(contact_id))?;
        for item in items {
            if matches!(
                self.storage.get_message_status(&item.id)?,
                Some(status) if status == "queued_mailbox"
            ) {
                self.storage.delete_outbox(&item.id)?;
                continue;
            }
            let plain = base64::engine::general_purpose::STANDARD
                .decode(&item.body)
                .unwrap_or_else(|_| item.body.as_bytes().to_vec());
            let payload = bytes_to_payload(&plain);
            if prefers_mailbox_delivery(&payload, plain.len()) {
                continue;
            }
            let display = message_record_from_payload(
                &item.id,
                &item.contact_id,
                "out",
                "pending",
                &payload,
            );
            if self
                .try_send_payload_bytes(&item.contact_id, &plain, &item.id, &display)
                .await?
                .is_some()
            {
                if !matches!(
                    self.storage.get_message_status(&item.id)?,
                    Some(status) if status == "queued_mailbox"
                ) {
                    self.storage.update_message_status(&item.id, "sent")?;
                }
                self.storage.delete_outbox(&item.id)?;
            }
        }
        Ok(())
    }

    /// Pull mailbox once; errors on one entry do not block others.
    pub async fn sync_all_mailboxes(&self) -> Result<Vec<MessageRecord>> {
        if !self.config.firebase_configured() {
            return Ok(vec![]);
        }
        let identity = self.identity.as_ref().context("no identity")?.clone();
        let me = self.my_user_id()?;
        let fb = self.firebase()?;

        if let Ok(pings) = fb.list_mailbox_pings(&me).await {
            for (from_id, _) in pings {
                fb.delete_mailbox_ping(&me, &from_id).await.ok();
            }
        }

        let known_contacts: HashSet<String> = self
            .storage
            .list_contacts()?
            .into_iter()
            .map(|contact| contact.user_id)
            .collect();
        let entries = fb.list_mailbox(&me).await?;
        let mut received = Vec::new();
        for (msg_id, entry) in entries {
            if !known_contacts.contains(&entry.from) {
                continue;
            }
            if let Ok(Some(record)) = self
                .process_mailbox_entry(&msg_id, &entry, &fb, &identity, &me)
                .await
            {
                received.push(record);
            }
        }
        Ok(received)
    }

    /// WebRTC connect maintenance — isolated from mailbox sync so delivery never blocks on ICE.
    pub async fn poll_connectivity(&self) -> Result<Option<String>> {
        self.recover_stale_handshakes().await;
        self.maybe_heartbeat_presence().await;
        let _ = self.handle_connect_pings().await;
        self.maybe_sync_contact_presence().await;
        let _ = self.maintain_wanted_connection().await;

        if let Ok(Some(contact_id)) = self.poll_signaling().await {
            let _ = self.advance_pending_answer().await;
            let _ = self.sync_mailbox(&contact_id).await;
            let _ = self.flush_outbox(&contact_id).await;
            return Ok(Some(contact_id));
        }

        if let Ok(Some(contact_id)) = self.advance_pending_answer().await {
            let _ = self.sync_mailbox(&contact_id).await;
            let _ = self.flush_outbox(&contact_id).await;
            return Ok(Some(contact_id));
        }

        if let Ok(Some(contact_id)) = self.advance_pending_offer().await {
            let _ = self.sync_mailbox(&contact_id).await;
            let _ = self.flush_outbox(&contact_id).await;
            return Ok(Some(contact_id));
        }

        if let Ok(me) = self.my_user_id() {
            let mut pending = self.pending_offer.lock().await;
            if let Some(offer) = pending.as_mut() {
                let contact_id = offer.contact_id.clone();
                let _ = self
                    .exchange_ice(&offer.peer, &me, &contact_id, &mut offer.seen_ice)
                    .await;
            }
        }

        let live_id = self.live_contact_id.read().ok().and_then(|g| g.clone());
        if let Some(contact_id) = live_id {
            let _ = self.flush_outbox(&contact_id).await;
        }

        Ok(None)
    }

    pub async fn recv_live_messages(&self) -> Result<Vec<MessageRecord>> {
        let mut received = Vec::new();
        let mut guard = self.active.lock().await;
        let Some(active) = guard.as_mut() else {
            return Ok(received);
        };

        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(50),
                active.peer.recv(),
            )
            .await
            {
                Ok(Some(bytes)) => {
                    if let WireMessage::EncryptedChat { ciphertext } =
                        WireMessage::from_bytes(&bytes)?
                    {
                        let plain = active.session.decrypt(&ciphertext)?;
                        let msg_id = Storage::new_message_id();
                        let record =
                            self.ingest_incoming_plain(&msg_id, &active.contact_id, &plain)?;
                        received.push(record);
                    }
                }
                _ => break,
            }
        }
        Ok(received)
    }

    pub async fn poll_incoming(&self) -> Result<Vec<MessageRecord>> {
        let _ = self.sync_invitations().await;
        let mut received = self.sync_all_mailboxes().await.unwrap_or_default();
        let _ = self.poll_connectivity().await;
        received.extend(self.recv_live_messages().await.unwrap_or_default());
        Ok(received)
    }

    async fn run_session_handshake_as_initiator(
        &self,
        peer: &mut PeerConnection,
        identity: Identity,
        remote_bundle: &PreKeyBundle,
    ) -> Result<Session> {
        let (initiator, init) = SessionInitiator::begin(identity, remote_bundle)?;
        peer.send(&WireMessage::SessionInit(init).to_bytes()?).await?;
        let ack_bytes = recv_bytes(peer, 30).await?;
        let ack = match WireMessage::from_bytes(&ack_bytes)? {
            WireMessage::SessionAck(v) => v,
            other => anyhow::bail!("unexpected {other:?}"),
        };
        Ok(initiator.complete(&ack)?)
    }

    async fn run_session_handshake_as_responder(
        &self,
        peer: &mut PeerConnection,
        identity: Identity,
        _remote_bundle: &PreKeyBundle,
    ) -> Result<Session> {
        let init_bytes = recv_bytes(peer, 30).await?;
        let init = match WireMessage::from_bytes(&init_bytes)? {
            WireMessage::SessionInit(v) => v,
            other => anyhow::bail!("unexpected {other:?}"),
        };
        let (responder, ack) = SessionResponder::accept(identity, &init)?;
        peer.send(&WireMessage::SessionAck(ack).to_bytes()?).await?;
        Ok(responder.complete(&init)?)
    }

    fn load_identity(&mut self) -> Result<()> {
        let path = self.data_dir.join("identity.json");
        if path.exists() {
            let json = std::fs::read_to_string(path)?;
            self.identity = Some(Identity::load_json(&json)?);
        }
        Ok(())
    }

    fn save_identity(&self, identity: &Identity) -> Result<()> {
        std::fs::write(self.data_dir.join("identity.json"), identity.save_json()?)?;
        self.storage.set_meta("user_id", &identity.public.user_id)?;
        Ok(())
    }

    fn avatar_path(&self) -> PathBuf {
        self.data_dir.join("avatar")
    }

    fn contact_avatar_path(&self, contact_id: &str) -> PathBuf {
        self.data_dir.join("contact_avatars").join(contact_id)
    }

    fn load_avatar_bytes(&self) -> Result<Option<Vec<u8>>> {
        let path = self.avatar_path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(path)?;
        if bytes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(bytes))
        }
    }

    pub fn contacts_with_avatars(&self) -> Result<Vec<ContactRecord>> {
        let mut contacts = self.storage.list_contacts()?;
        for c in &mut contacts {
            c.avatar_data_url = self.load_contact_avatar_data_url(&c.user_id);
        }
        Ok(contacts)
    }

    fn load_contact_avatar_data_url(&self, contact_id: &str) -> Option<String> {
        let path = self.contact_avatar_path(contact_id);
        if !path.exists() {
            return None;
        }
        let mime = self
            .storage
            .get_meta(&format!("contact_avatar_mime:{contact_id}"))
            .ok()
            .flatten()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "image/png".into());
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() {
            return None;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("data:{mime};base64,{b64}"))
    }

    fn save_contact_avatar(&self, contact_id: &str, bytes: &[u8], mime: &str) -> Result<()> {
        let dir = self.data_dir.join("contact_avatars");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(self.contact_avatar_path(contact_id), bytes)?;
        self.storage
            .set_meta(&format!("contact_avatar_mime:{contact_id}"), mime)?;
        Ok(())
    }

    fn load_avatar_data_url(&self) -> Option<String> {
        let path = self.avatar_path();
        if !path.exists() {
            return None;
        }
        let mime = self
            .storage
            .get_meta("avatar_mime")
            .ok()
            .flatten()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "image/png".into());
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() {
            return None;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("data:{mime};base64,{b64}"))
    }

    fn clear_avatar(&self) -> Result<()> {
        let path = self.avatar_path();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        self.storage.set_meta("avatar_mime", "")?;
        Ok(())
    }

    fn save_avatar(&self, data_url: &str) -> Result<()> {
        const MAX_BYTES: usize = 512 * 1024;

        let (mime, bytes) = parse_image_data_url(data_url)?;
        if bytes.len() > MAX_BYTES {
            anyhow::bail!("avatar too large (max 512 KB)");
        }
        std::fs::write(self.avatar_path(), &bytes)?;
        self.storage.set_meta("avatar_mime", &mime)?;
        Ok(())
    }
}

fn glare_key(user_id: &str) -> String {
    user_id.to_ascii_lowercase()
}

fn should_initiate_offer(me: &str, contact_id: &str) -> bool {
    glare_key(me) <= glare_key(contact_id)
}

fn normalize_user_id(user_id: &str) -> Result<String> {
    let id = user_id.trim().trim_start_matches('@');
    if id.is_empty() {
        anyhow::bail!("user ID cannot be empty");
    }
    if id.chars().count() > 64 {
        anyhow::bail!("user ID too long (max 64)");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        anyhow::bail!("user ID may only contain letters, numbers, _, -, .");
    }
    Ok(id.to_string())
}

fn parse_image_data_url(data_url: &str) -> Result<(String, Vec<u8>)> {
    let trimmed = data_url.trim();
    if trimmed.starts_with("data:") {
        let rest = trimmed.strip_prefix("data:").unwrap();
        let (meta, payload) = rest
            .split_once(',')
            .context("invalid avatar data URL")?;
        let mime = meta
            .split(';')
            .next()
            .unwrap_or("image/png")
            .to_string();
        if !mime.starts_with("image/") {
            anyhow::bail!("avatar must be an image");
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(payload)?;
        return Ok((mime, bytes));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(trimmed)?;
    Ok(("image/png".into(), bytes))
}

async fn recv_bytes(peer: &mut PeerConnection, secs: u64) -> Result<Vec<u8>> {
    tokio::time::timeout(std::time::Duration::from_secs(secs), peer.recv())
        .await
        .context("timeout")?
        .context("connection closed")
}

pub type SharedApp = Arc<tokio::sync::RwLock<CorgigramApp>>;
