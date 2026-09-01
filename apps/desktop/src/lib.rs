mod updater;

use std::sync::Arc;

use corgigram_core::{AppConfig, AppSnapshot, ConnectAnswerResult, ConnectAutoResult, ConnectDiagnose, ConnectOfferResult, CorgigramApp, ProfileInfo, SharedApp};
use corgigram_storage::{ContactRecord, MessageRecord};
use tauri::{Emitter, State};

struct AppState {
    app: SharedApp,
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    state.app.read().await.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_identity(
    state: State<'_, AppState>,
    user_id: String,
    display_name: String,
) -> Result<ProfileInfo, String> {
    let profile = state
        .app
        .write()
        .await
        .create_identity(&user_id, &display_name)
        .map_err(|e| e.to_string())?;
    let _ = state.app.read().await.sync_directory().await;
    Ok(profile)
}

#[tauri::command]
async fn get_bundle_qr(state: State<'_, AppState>) -> Result<String, String> {
    state
        .app
        .read()
        .await
        .bundle_qr_png_base64()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_contact(state: State<'_, AppState>, bundle_json: String) -> Result<ContactRecord, String> {
    let owner_id = {
        let mut app = state.app.write().await;
        let contact = app
            .add_contact_from_bundle_json(&bundle_json)
            .map_err(|e| e.to_string())?;
        contact.user_id.clone()
    };
    let app = state.app.read().await;
    let _ = app.request_contact_avatar(&owner_id).await;
    let _ = app.sync_avatar_downloads().await;
    app.contacts_with_avatars()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.user_id == owner_id)
        .ok_or_else(|| "contact missing".to_string())
}

#[tauri::command]
async fn add_contact_by_id(state: State<'_, AppState>, user_id: String) -> Result<ContactRecord, String> {
    state
        .app
        .write()
        .await
        .add_contact_by_user_id(&user_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_directory(state: State<'_, AppState>) -> Result<(), String> {
    state
        .app
        .read()
        .await
        .sync_directory()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_avatars(state: State<'_, AppState>) -> Result<(), String> {
    state
        .app
        .read()
        .await
        .sync_avatars()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn accept_invitation(state: State<'_, AppState>, from_user_id: String) -> Result<ContactRecord, String> {
    state
        .app
        .write()
        .await
        .accept_invitation(&from_user_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn decline_invitation(state: State<'_, AppState>, from_user_id: String) -> Result<(), String> {
    state
        .app
        .read()
        .await
        .decline_invitation(&from_user_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_messages(state: State<'_, AppState>, contact_id: String) -> Result<Vec<MessageRecord>, String> {
    state
        .app
        .read()
        .await
        .messages(&contact_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_safety_number(state: State<'_, AppState>, contact_id: String) -> Result<String, String> {
    state
        .app
        .read()
        .await
        .safety_number(&contact_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_wanted_contact(state: State<'_, AppState>, contact_id: Option<String>) -> Result<(), String> {
    state
        .app
        .read()
        .await
        .set_wanted_contact(contact_id)
        .await;
    Ok(())
}

#[tauri::command]
async fn connect_offer(state: State<'_, AppState>, contact_id: String) -> Result<ConnectOfferResult, String> {
    state
        .app
        .read()
        .await
        .connect_offer(&contact_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_auto(state: State<'_, AppState>, contact_id: String) -> Result<ConnectAutoResult, String> {
    state
        .app
        .read()
        .await
        .connect_auto(&contact_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn diagnose_connect(
    state: State<'_, AppState>,
    contact_id: String,
) -> Result<ConnectDiagnose, String> {
    state
        .app
        .read()
        .await
        .diagnose_connect(&contact_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_mailbox(state: State<'_, AppState>, contact_id: String) -> Result<Vec<MessageRecord>, String> {
    state
        .app
        .read()
        .await
        .sync_mailbox(&contact_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_finish(
    state: State<'_, AppState>,
    contact_id: String,
    answer_sdp: String,
) -> Result<(), String> {
    state
        .app
        .read()
        .await
        .connect_finish(&contact_id, &answer_sdp)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_answer(
    state: State<'_, AppState>,
    contact_id: String,
    offer_sdp: String,
) -> Result<ConnectAnswerResult, String> {
    state
        .app
        .read()
        .await
        .connect_answer(&offer_sdp, &contact_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_message(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    contact_id: String,
    text: String,
) -> Result<MessageRecord, String> {
    let record = state
        .app
        .read()
        .await
        .send_message(&contact_id, &text)
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("message-sent", &record);
    Ok(record)
}

#[tauri::command]
async fn poll_messages(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Vec<MessageRecord>, String> {
    let incoming = state
        .app
        .read()
        .await
        .poll_incoming()
        .await
        .map_err(|e| e.to_string())?;
    for msg in &incoming {
        let _ = app.emit("message-received", msg);
    }
    Ok(incoming)
}

#[tauri::command]
async fn save_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    state
        .app
        .write()
        .await
        .update_config(config)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_profile(
    state: State<'_, AppState>,
    display_name: Option<String>,
    avatar_data_url: Option<String>,
    remove_avatar: Option<bool>,
) -> Result<ProfileInfo, String> {
    state
        .app
        .write()
        .await
        .update_profile(
            display_name.as_deref(),
            avatar_data_url.as_deref(),
            remove_avatar.unwrap_or(false),
        )
        .map_err(|e| e.to_string())?;
    let profile = state
        .app
        .read()
        .await
        .profile_info()
        .ok_or_else(|| "profile missing".to_string())?;
    let _ = state.app.read().await.sync_directory().await;
    Ok(profile)
}

pub fn run() {
    let corgigram = CorgigramApp::open_default().expect("failed to open app data");
    let shared: SharedApp = Arc::new(tokio::sync::RwLock::new(corgigram));
    let poll_app = shared.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState { app: shared })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            create_identity,
            get_bundle_qr,
            add_contact,
            add_contact_by_id,
            accept_invitation,
            decline_invitation,
            sync_directory,
            sync_avatars,
            get_messages,
            get_safety_number,
            set_wanted_contact,
            connect_offer,
            connect_auto,
            diagnose_connect,
            connect_finish,
            connect_answer,
            sync_mailbox,
            send_message,
            poll_messages,
            save_config,
            update_profile,
        ])
        .setup(move |app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            #[cfg(all(desktop, not(debug_assertions)))]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    updater::check_and_install(handle).await;
                });
            }

            let sync_app = poll_app.clone();
            tauri::async_runtime::spawn(async move {
                let app = sync_app.read().await;
                let _ = app.sync_directory().await;
                let _ = app.sync_avatar_downloads().await;
                app.prefetch_turn().await;
            });
            let avatar_poll = poll_app.clone();
            let avatar_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    let app = avatar_poll.read().await;
                    let _ = app.sync_avatars().await;
                    let _ = avatar_handle.emit("contacts-updated", ());
                }
            });
            let poll_handle = app.handle().clone();
            let message_poll = poll_app.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    let incoming = message_poll.read().await.poll_incoming().await.unwrap_or_default();
                    for msg in incoming {
                        let _ = poll_handle.emit("message-received", &msg);
                    }
                    let _ = poll_handle.emit("contacts-updated", ());
                }
            });
            let ice_poll = poll_app.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let app = ice_poll.read().await;
                    let _ = app.exchange_pending_ice().await;
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running Corgigram");
}
