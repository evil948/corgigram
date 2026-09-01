mod updater;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::Engine;
use corgigram_core::{
    AppConfig, AppSnapshot, AttachmentData, BackgroundTickResult, ConnectAnswerResult,
    ConnectAutoResult, ConnectDiagnose, ConnectOfferResult, CorgigramApp, OutgoingAttachment,
    ProfileInfo, SharedApp,
};
use corgigram_storage::{ContactRecord, MessageRecord};
use serde::Deserialize;
use tauri::{Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;

struct AppState {
    app: SharedApp,
}

static OFFLINE_MARKED: AtomicBool = AtomicBool::new(false);

async fn mark_offline(app: &SharedApp) {
    if OFFLINE_MARKED.swap(true, Ordering::SeqCst) {
        return;
    }
    app.read().await.go_offline().await.ok();
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentInput {
    name: String,
    mime: String,
    data_base64: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardAttachmentDto {
    mime: String,
    data_base64: String,
    name: String,
}

const MAX_CLIPBOARD_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_CLIPBOARD_ATTACHMENTS: usize = 10;

fn encode_rgba_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width as u32, height as u32, rgba.to_vec())
        .ok_or_else(|| "invalid clipboard image dimensions".to_string())?;
    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())?;
    Ok(buf)
}

fn guess_mime_from_name(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    };
    mime.to_string()
}

fn attachment_from_path(path: &std::path::Path) -> Result<ClipboardAttachmentDto, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("clipboard path is not a file".into());
    }
    if meta.len() > MAX_CLIPBOARD_ATTACHMENT_BYTES {
        return Err("clipboard file is too large".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clipboard-file")
        .to_string();
    Ok(ClipboardAttachmentDto {
        mime: guess_mime_from_name(&name),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        name,
    })
}

fn read_clipboard_attachments_inner() -> Result<Vec<ClipboardAttachmentDto>, String> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;

    match clipboard.get().file_list() {
        Ok(paths) if !paths.is_empty() => {
            let mut out = Vec::new();
            for path in paths.into_iter().take(MAX_CLIPBOARD_ATTACHMENTS) {
                if let Ok(item) = attachment_from_path(&path) {
                    out.push(item);
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
        Err(_) => {}
        Ok(_) => {}
    }

    match clipboard.get_image() {
        Ok(image) => {
            let png = encode_rgba_png(image.width, image.height, &image.bytes)?;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            Ok(vec![ClipboardAttachmentDto {
                mime: "image/png".into(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(png),
                name: format!("clipboard-{ts}.png"),
            }])
        }
        Err(arboard::Error::ContentNotAvailable) => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn read_clipboard_attachments() -> Result<Vec<ClipboardAttachmentDto>, String> {
    read_clipboard_attachments_inner()
}

#[tauri::command]
fn read_clipboard_image() -> Result<Option<ClipboardAttachmentDto>, String> {
    Ok(read_clipboard_attachments_inner()?.into_iter().next())
}

#[tauri::command]
fn read_clipboard_text() -> Result<Option<String>, String> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
    match clipboard.get_text() {
        Ok(text) if !text.is_empty() => Ok(Some(text)),
        Ok(_) => Ok(None),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    state.app.read().await.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
async fn mark_contact_read(state: State<'_, AppState>, contact_id: String) -> Result<(), String> {
    state
        .app
        .read()
        .await
        .mark_contact_read(&contact_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_contact_avatar(state: State<'_, AppState>, contact_id: String) -> Result<Option<String>, String> {
    Ok(state
        .app
        .read()
        .await
        .get_contact_avatar(&contact_id))
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
async fn get_messages_page(
    state: State<'_, AppState>,
    contact_id: String,
    before_created_at: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<MessageRecord>, String> {
    state
        .app
        .read()
        .await
        .messages_page(
            &contact_id,
            before_created_at.as_deref(),
            limit.unwrap_or(50),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn read_attachment(
    state: State<'_, AppState>,
    message_id: String,
    index: usize,
) -> Result<AttachmentData, String> {
    state
        .app
        .read()
        .await
        .read_attachment(&message_id, index)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_attachment_count(
    state: State<'_, AppState>,
    message_id: String,
) -> Result<usize, String> {
    Ok(state
        .app
        .read()
        .await
        .attachment_count(&message_id))
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
async fn send_attachments(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    contact_id: String,
    attachments: Vec<AttachmentInput>,
    caption: Option<String>,
) -> Result<MessageRecord, String> {
    let parsed = attachments
        .into_iter()
        .map(|item| {
            let data = base64::engine::general_purpose::STANDARD
                .decode(&item.data_base64)
                .map_err(|e| e.to_string())?;
            Ok(OutgoingAttachment {
                name: item.name,
                mime: item.mime,
                data,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let record = state
        .app
        .read()
        .await
        .send_attachments(&contact_id, parsed, caption.as_deref())
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

#[tauri::command]
async fn check_for_updates_manual(app: tauri::AppHandle) -> Result<String, String> {
    match updater::check_for_updates(&app).await {
        updater::UpdateCheckResult::UpToDate => {
            Ok(format!("У вас последняя версия ({}).", app.package_info().version))
        }
        updater::UpdateCheckResult::UpdateAvailable { version } => {
            updater::check_and_install(app).await;
            Ok(format!("Доступна версия {version}."))
        }
        updater::UpdateCheckResult::Failed(msg) => Err(msg),
    }
}

fn inbound_preview(msg: &MessageRecord) -> String {
    match msg.kind.as_str() {
        "image" => "Фото".into(),
        "album" => "Альбом".into(),
        "file" => msg
            .attachment_name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map(|n| format!("Файл · {n}"))
            .unwrap_or_else(|| "Файл".into()),
        _ => {
            let t = msg.body.trim();
            if t.chars().count() > 80 {
                format!("{}…", t.chars().take(80).collect::<String>())
            } else {
                t.to_string()
            }
        }
    }
}

async fn maybe_native_notify(app: &tauri::AppHandle, shared: &SharedApp, msg: &MessageRecord) {
    if msg.direction != "in" {
        return;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let snapshot = match shared.read().await.snapshot() {
        Ok(s) => s,
        Err(_) => return,
    };
    let in_active_chat = focused
        && snapshot
            .wanted_contact_id
            .as_deref()
            == Some(msg.contact_id.as_str());
    if in_active_chat {
        return;
    }
    let title = snapshot
        .contacts
        .iter()
        .find(|c| c.user_id == msg.contact_id)
        .map(|c| c.display_name.as_str())
        .unwrap_or(&msg.contact_id);
    let body = inbound_preview(msg);
    show_desktop_notification(app, title, &body);
}

fn show_desktop_notification(app: &tauri::AppHandle, title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = notify_rust::Notification::new()
            .appname("korki")
            .summary(title)
            .body(body)
            .show();
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show();
    }
}

pub fn run() {
    let corgigram = CorgigramApp::open_default().expect("failed to open app data");
    let shared: SharedApp = Arc::new(tokio::sync::RwLock::new(corgigram));
    let poll_app = shared.clone();
    let exit_app = shared.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState { app: shared })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            mark_contact_read,
            get_contact_avatar,
            create_identity,
            get_bundle_qr,
            add_contact,
            add_contact_by_id,
            accept_invitation,
            decline_invitation,
            sync_directory,
            sync_avatars,
            get_messages,
            get_messages_page,
            read_attachment,
            get_attachment_count,
            get_safety_number,
            set_wanted_contact,
            connect_offer,
            connect_auto,
            diagnose_connect,
            connect_finish,
            connect_answer,
            sync_mailbox,
            send_message,
            send_attachments,
            poll_messages,
            save_config,
            update_profile,
            read_clipboard_attachments,
            read_clipboard_image,
            read_clipboard_text,
            check_for_updates_manual,
        ])
        .setup(move |app| {
            #[cfg(all(not(debug_assertions), not(any(target_os = "android", target_os = "ios"))))]
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
                let _ = app.announce_online().await;
                let _ = app.sync_avatar_downloads().await;
                app.prefetch_turn().await;
            });

            let tick_app = poll_app.clone();
            let tick_handle = app.handle().clone();
            let offline_on_close = poll_app.clone();
            if let Some(window) = app.get_webview_window("main") {
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Destroyed) {
                        let app = offline_on_close.clone();
                        tauri::async_runtime::spawn(async move {
                            mark_offline(&app).await;
                        });
                    }
                });
            }
            tauri::async_runtime::spawn(async move {
                let mut avatar_counter = 0u32;
                let mut sleep_ms = 250u64;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                    avatar_counter = avatar_counter.wrapping_add(1);
                    let tick = tick_app.read().await.background_tick().await;
                    let Ok(BackgroundTickResult {
                        messages,
                        contacts_changed,
                        status_updates,
                        connecting,
                    }) = tick
                    else {
                        continue;
                    };
                    sleep_ms = if connecting { 50 } else { 200 };
                    for msg in &messages {
                        let _ = tick_handle.emit("message-received", msg);
                        maybe_native_notify(&tick_handle, &tick_app, msg).await;
                    }
                    if !messages.is_empty() {
                        let _ = tick_handle.emit("messages-updated", &messages);
                    }
                    for (msg_id, status) in status_updates {
                        let _ = tick_handle.emit(
                            "message-status-updated",
                            serde_json::json!({ "id": msg_id, "status": status }),
                        );
                    }
                    if contacts_changed {
                        let _ = tick_handle.emit("contacts-updated", ());
                    }
                    if avatar_counter % 60 == 0 {
                        let app = tick_app.read().await;
                        let _ = app.sync_avatars().await;
                        let _ = tick_handle.emit("contacts-updated", ());
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error running korki")
        .run(move |_app_handle, event| {
            if let RunEvent::Exit = event {
                tauri::async_runtime::block_on(async {
                    mark_offline(&exit_app).await;
                });
            }
        });
}
