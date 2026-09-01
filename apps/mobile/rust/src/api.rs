use std::path::PathBuf;
use std::sync::Arc;

use corgigram_core::CorgigramApp;
use flutter_rust_bridge::frb;
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;

use crate::dto::{
    config_from, ConnectAutoDto, ContactDto, MessageDto, ProfileDto, PushPayloadDto, SnapshotDto,
};

static APP: OnceCell<Arc<Mutex<CorgigramApp>>> = OnceCell::new();

fn app() -> Result<&'static Arc<Mutex<CorgigramApp>>, String> {
    APP.get().ok_or_else(|| "korki not initialized".to_string())
}

#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

#[frb(sync)]
pub fn corgigram_init(data_dir: String) -> Result<(), String> {
    if APP.get().is_some() {
        return Ok(());
    }
    let core = CorgigramApp::open(PathBuf::from(data_dir)).map_err(|e| e.to_string())?;
    APP.set(Arc::new(Mutex::new(core)))
        .map_err(|_| "already initialized".to_string())
}

#[frb]
pub async fn get_snapshot() -> Result<SnapshotDto, String> {
    app()?
        .lock()
        .await
        .snapshot()
        .map(SnapshotDto::from)
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn create_identity(user_id: String, display_name: String) -> Result<ProfileDto, String> {
    let profile = app()?
        .lock()
        .await
        .create_identity(&user_id, &display_name)
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())?;
    let _ = app()?.lock().await.sync_directory().await;
    Ok(profile)
}

#[frb]
pub async fn get_bundle_qr() -> Result<String, String> {
    app()?
        .lock()
        .await
        .bundle_qr_png_base64()
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn add_contact(bundle_json: String) -> Result<ContactDto, String> {
    app()?
        .lock()
        .await
        .add_contact_from_bundle_json(&bundle_json)
        .map(ContactDto::from)
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn add_contact_by_id(user_id: String) -> Result<ContactDto, String> {
    app()?
        .lock()
        .await
        .add_contact_by_user_id(&user_id)
        .await
        .map(ContactDto::from)
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn sync_directory() -> Result<(), String> {
    app()?
        .lock()
        .await
        .sync_directory()
        .await
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn get_messages(contact_id: String) -> Result<Vec<MessageDto>, String> {
    app()?
        .lock()
        .await
        .messages(&contact_id)
        .map(|msgs| msgs.into_iter().map(MessageDto::from).collect())
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn get_safety_number(contact_id: String) -> Result<String, String> {
    app()?
        .lock()
        .await
        .safety_number(&contact_id)
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn connect_auto(contact_id: String) -> Result<ConnectAutoDto, String> {
    app()?
        .lock()
        .await
        .connect_auto(&contact_id)
        .await
        .map(ConnectAutoDto::from)
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn sync_mailbox(contact_id: String) -> Result<Vec<MessageDto>, String> {
    app()?
        .lock()
        .await
        .sync_mailbox(&contact_id)
        .await
        .map(|msgs| msgs.into_iter().map(MessageDto::from).collect())
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn send_message(contact_id: String, text: String) -> Result<MessageDto, String> {
    app()?
        .lock()
        .await
        .send_message(&contact_id, &text)
        .await
        .map(MessageDto::from)
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn poll_incoming() -> Result<Vec<MessageDto>, String> {
    app()?
        .lock()
        .await
        .poll_incoming()
        .await
        .map(|msgs| msgs.into_iter().map(MessageDto::from).collect())
        .map_err(|e| e.to_string())
}

#[frb]
pub async fn save_config(
    firebase_database_url: Option<String>,
    firebase_auth_token: Option<String>,
) -> Result<(), String> {
    app()?
        .lock()
        .await
        .update_config(config_from(firebase_database_url, firebase_auth_token))
        .map_err(|e| e.to_string())
}

#[frb(sync)]
pub fn push_payload_new_message(sender_id: String) -> PushPayloadDto {
    corgigram_core::PushNotification::new_message(&sender_id).into()
}

#[frb(sync)]
pub fn default_firebase_url() -> String {
    corgigram_core::DEFAULT_FIREBASE_DATABASE_URL.to_string()
}
