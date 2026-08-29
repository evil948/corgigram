use tauri::Emitter;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

/// Check GitHub releases for a new version; install and restart if the user agrees.
pub async fn check_and_install(app: tauri::AppHandle) {
    let Ok(updater) = app.updater() else {
        return;
    };

    let Ok(Some(update)) = updater.check().await else {
        return;
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
        .title("Обновление Corgigram")
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
            .title("Обновление Corgigram")
            .kind(MessageDialogKind::Error)
            .blocking_show();
        return;
    }

    app.restart();
}
