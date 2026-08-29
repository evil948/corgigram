fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "get_snapshot",
                "create_identity",
                "get_bundle_qr",
                "add_contact",
                "add_contact_by_id",
                "sync_directory",
                "sync_avatars",
                "get_messages",
                "get_safety_number",
                "connect_offer",
                "connect_auto",
                "connect_finish",
                "connect_answer",
                "sync_mailbox",
                "send_message",
                "poll_messages",
                "save_config",
                "update_profile",
            ]),
        ),
    )
    .expect("failed to run tauri build");
}
