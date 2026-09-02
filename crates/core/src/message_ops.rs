use std::path::Path;

use anyhow::{bail, Context, Result};
use corgigram_protocol::{ChatEnvelope, MessageOp};
use corgigram_storage::{MessageRecord, Storage};

use crate::chat_media::{
    message_record_from_payload, save_payload_attachments, save_payload_attachments_if_needed,
};

pub const DELETED_BODY: &str = "Сообщение удалено";

pub fn envelope_to_bytes(envelope: &ChatEnvelope) -> Result<Vec<u8>> {
    envelope
        .to_bytes()
        .map_err(|error| anyhow::anyhow!("envelope encode: {error}"))
}

pub fn bytes_to_envelope(bytes: &[u8], fallback_id: &str) -> ChatEnvelope {
    ChatEnvelope::from_legacy(bytes, fallback_id)
}

pub fn apply_incoming_envelope(
    storage: &Storage,
    data_dir: &Path,
    contact_id: &str,
    envelope: &ChatEnvelope,
) -> Result<MessageRecord> {
    if storage.event_exists(&envelope.id)? {
        let msg_id = envelope.message_id();
        return storage
            .get_message(msg_id)?
            .context("event recorded but message missing");
    }

    let record = match envelope.op {
        MessageOp::Create => apply_create(storage, data_dir, contact_id, envelope)?,
        MessageOp::Edit => apply_edit(storage, data_dir, contact_id, envelope)?,
        MessageOp::Delete => apply_delete(storage, contact_id, envelope)?,
    };

    storage.insert_message_event(
        &envelope.id,
        record.id.as_str(),
        op_name(envelope.op),
    )?;
    Ok(record)
}

fn apply_create(
    storage: &Storage,
    data_dir: &Path,
    contact_id: &str,
    envelope: &ChatEnvelope,
) -> Result<MessageRecord> {
    let msg_id = envelope.message_id();
    if let Some(existing) = storage.get_message(msg_id)? {
        return Ok(existing);
    }
    let payload = envelope
        .payload
        .clone()
        .context("create envelope missing payload")?;
    save_payload_attachments(data_dir, msg_id, &payload)?;
    let mut record =
        message_record_from_payload(msg_id, contact_id, "in", "delivered", &payload);
    record.created_at = envelope.ts;
    storage.upsert_message(&record)?;
    Ok(record)
}

fn apply_edit(
    storage: &Storage,
    data_dir: &Path,
    contact_id: &str,
    envelope: &ChatEnvelope,
) -> Result<MessageRecord> {
    let target_id = envelope
        .target_id
        .as_deref()
        .context("edit envelope missing target_id")?;
    let payload = envelope
        .payload
        .clone()
        .context("edit envelope missing payload")?;
    let mut record = storage
        .get_message(target_id)?
        .with_context(|| format!("edit target {target_id} not found"))?;
    if record.contact_id != contact_id {
        bail!("edit target belongs to another contact");
    }
    save_payload_attachments_if_needed(data_dir, target_id, &payload)?;
    let updated = message_record_from_payload(
        target_id,
        contact_id,
        record.direction.as_str(),
        record.status.as_str(),
        &payload,
    );
    record.body = updated.body;
    record.kind = updated.kind;
    record.attachment_name = updated.attachment_name;
    record.attachment_mime = updated.attachment_mime;
    record.edited_at = Some(envelope.ts);
    record.revision = record.revision.saturating_add(1);
    record.deleted_at = None;
    storage.upsert_message(&record)?;
    Ok(record)
}

fn apply_delete(storage: &Storage, contact_id: &str, envelope: &ChatEnvelope) -> Result<MessageRecord> {
    let target_id = envelope
        .target_id
        .as_deref()
        .context("delete envelope missing target_id")?;
    let mut record = storage
        .get_message(target_id)?
        .with_context(|| format!("delete target {target_id} not found"))?;
    if record.contact_id != contact_id {
        bail!("delete target belongs to another contact");
    }
    record.body = DELETED_BODY.into();
    record.kind = "deleted".into();
    record.attachment_name = None;
    record.attachment_mime = None;
    record.deleted_at = Some(envelope.ts);
    record.revision = record.revision.saturating_add(1);
    storage.upsert_message(&record)?;
    Ok(record)
}

fn op_name(op: MessageOp) -> &'static str {
    match op {
        MessageOp::Create => "create",
        MessageOp::Edit => "edit",
        MessageOp::Delete => "delete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use corgigram_crypto::PreKeyBundle;
    use chrono::Utc;
    use corgigram_protocol::ChatPayload;
    use tempfile::tempdir;

    fn test_bundle(user_id: &str) -> PreKeyBundle {
        use corgigram_crypto::Identity;
        Identity::generate(user_id, user_id).prekey_bundle()
    }

    #[test]
    fn create_and_edit_text_message() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("t.db")).unwrap();
        storage
            .upsert_contact(&corgigram_storage::ContactRecord {
                user_id: "c1".into(),
                display_name: "C".into(),
                bundle: test_bundle("c1"),
                created_at: Utc::now(),
                avatar_data_url: None,
            })
            .unwrap();

        let create = ChatEnvelope::create(
            "msg-1",
            ChatPayload::Text {
                body: "hello".into(),
            },
        );
        let record = apply_incoming_envelope(&storage, dir.path(), "c1", &create).unwrap();
        assert_eq!(record.body, "hello");

        let edit = ChatEnvelope::edit(
            "evt-1",
            "msg-1",
            ChatPayload::Text {
                body: "edited".into(),
            },
        );
        let updated = apply_incoming_envelope(&storage, dir.path(), "c1", &edit).unwrap();
        assert_eq!(updated.body, "edited");
        assert!(updated.edited_at.is_some());
        assert_eq!(updated.revision, 1);

        let again = apply_incoming_envelope(&storage, dir.path(), "c1", &edit).unwrap();
        assert_eq!(again.body, "edited");
    }

    #[test]
    fn delete_message_tombstone() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("t.db")).unwrap();
        storage
            .upsert_contact(&corgigram_storage::ContactRecord {
                user_id: "c1".into(),
                display_name: "C".into(),
                bundle: test_bundle("c1"),
                created_at: Utc::now(),
                avatar_data_url: None,
            })
            .unwrap();
        let create = ChatEnvelope::create(
            "msg-2",
            ChatPayload::Text {
                body: "bye".into(),
            },
        );
        apply_incoming_envelope(&storage, dir.path(), "c1", &create).unwrap();
        let delete = ChatEnvelope::delete("evt-del", "msg-2");
        let tomb = apply_incoming_envelope(&storage, dir.path(), "c1", &delete).unwrap();
        assert_eq!(tomb.kind, "deleted");
        assert_eq!(tomb.body, DELETED_BODY);
        assert!(tomb.deleted_at.is_some());
    }
}
