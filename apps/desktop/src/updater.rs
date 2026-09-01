use tauri::Emitter;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug)]
pub enum UpdateCheckResult {
    UpToDate,
    UpdateAvailable { version: String },
    Failed(String),
}

fn linux_appimage_ready() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("APPIMAGE").is_err() {
            return Err(
                "Автообновление работает только при запуске из AppImage (.AppImage), не из cargo build или установленного бинарника.".into(),
            );
        }
    }
    Ok(())
}

pub async fn check_for_updates(app: &tauri::AppHandle) -> UpdateCheckResult {
    if let Err(reason) = linux_appimage_ready() {
        return UpdateCheckResult::Failed(reason);
    }

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(err) => return UpdateCheckResult::Failed(format!("Плагин обновлений недоступен: {err}")),
    };

    match updater.check().await {
        Ok(Some(update)) => {
            let current = app.package_info().version.to_string();
            if update.version == current {
                UpdateCheckResult::UpToDate
            } else {
                UpdateCheckResult::UpdateAvailable {
                    version: update.version,
                }
            }
        }
        Ok(None) => UpdateCheckResult::UpToDate,
        Err(err) => UpdateCheckResult::Failed(format!("Проверка обновлений не удалась: {err}")),
    }
}

/// Check GitHub releases for a new version; install and restart if the user agrees.
pub async fn check_and_install(app: tauri::AppHandle) {
    if let Err(reason) = linux_appimage_ready() {
        eprintln!("OTA: {reason}");
        return;
    }

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(err) => {
            eprintln!("OTA: updater unavailable: {err}");
            return;
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => return,
        Err(err) => {
            eprintln!("OTA check failed: {err}");
            return;
        }
    };

    let current = app.package_info().version.to_string();
    if update.version == current {
        return;
    }

    let message = format!(
        "Доступна версия {} (сейчас {}).\n\nУстановить обновление и перезапустить?",
        update.version, current
    );

    let install = app
        .dialog()
        .message(message)
        .title("Обновление korki")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Установить".into(),
            "Позже".into(),
        ))
        .blocking_show();

    if !install {
        return;
    }

    let progress = app.clone();
    if let Err(err) = update
        .download_and_install(
            move |chunk, total| {
                let _ = progress.emit("update-progress", (chunk, total));
            },
            || {},
        )
        .await
    {
        let _ = app
            .dialog()
            .message(format!("Не удалось установить обновление:\n{err}"))
            .title("Обновление korki")
            .kind(MessageDialogKind::Error)
            .blocking_show();
        return;
    }

    app.restart();
}
