use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine;
use corgigram_protocol::{AttachmentItem, ChatPayload};
use corgigram_storage::MessageRecord;
use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 10;
/// WebRTC SCTP data channel safe payload budget (plain bytes before encryption).
pub const LIVE_PLAIN_MAX_BYTES: usize = 48_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingAttachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentData {
    pub name: String,
    pub mime: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMeta {
    name: String,
    mime: String,
}

impl OutgoingAttachment {
    pub fn validate(&self) -> Result<()> {
        if self.data.is_empty() {
            bail!("empty attachment");
        }
        if self.data.len() > MAX_ATTACHMENT_BYTES {
            bail!("файл «{}» больше 20 МБ", self.name);
        }
        Ok(())
    }
}

pub fn build_payload(text: Option<&str>, attachments: &[OutgoingAttachment]) -> Result<ChatPayload> {
    if attachments.is_empty() {
        let body = text.unwrap_or("").trim();
        if body.is_empty() {
            bail!("пустое сообщение");
        }
        return Ok(ChatPayload::Text {
            body: body.to_string(),
        });
    }
    if attachments.len() > MAX_ATTACHMENTS {
        bail!("слишком много вложений (макс. {MAX_ATTACHMENTS})");
    }
    for attachment in attachments {
        attachment.validate()?;
    }
    let caption = text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if attachments.len() == 1 {
        let attachment = &attachments[0];
        return Ok(ChatPayload::File {
            name: attachment.name.clone(),
            mime: attachment.mime.clone(),
            data: attachment.data.clone(),
            caption,
            keep_attachment: false,
        });
    }
    Ok(ChatPayload::Album {
        caption,
        items: attachments
            .iter()
            .map(|attachment| AttachmentItem {
                name: attachment.name.clone(),
                mime: attachment.mime.clone(),
                data: attachment.data.clone(),
            })
            .collect(),
    })
}

pub fn payload_to_bytes(payload: &ChatPayload) -> Result<Vec<u8>> {
    payload
        .to_bytes()
        .map_err(|error| anyhow::anyhow!("payload encode: {error}"))
}

pub fn bytes_to_payload(bytes: &[u8]) -> ChatPayload {
    ChatPayload::from_bytes(bytes).unwrap_or_else(|_| ChatPayload::from_legacy_plaintext(bytes))
}

/// Media and large payloads must use Firebase mailbox — SCTP data channel cannot carry them.
pub fn prefers_mailbox_delivery(payload: &ChatPayload, plain_len: usize) -> bool {
    match payload {
        ChatPayload::Text { .. } => plain_len > LIVE_PLAIN_MAX_BYTES,
        ChatPayload::File { .. } | ChatPayload::Album { .. } => true,
    }
}

pub fn message_record_from_payload(
    id: &str,
    contact_id: &str,
    direction: &str,
    status: &str,
    payload: &ChatPayload,
) -> MessageRecord {
    let (kind, body, attachment_name, attachment_mime) = match payload {
        ChatPayload::Text { body } => ("text".into(), body.clone(), None, None),
        ChatPayload::File {
            name,
            mime,
            caption,
            ..
        } => {
            let body = caption.clone().unwrap_or_else(|| name.clone());
            let kind = if mime.starts_with("image/") {
                "image"
            } else {
                "file"
            };
            (
                kind.into(),
                body,
                Some(name.clone()),
                Some(mime.clone()),
            )
        }
        ChatPayload::Album { caption, items } => {
            let names = items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let body = caption
                .clone()
                .unwrap_or_else(|| format!("{} файлов", items.len()));
            (
                "album".into(),
                body,
                Some(names),
                items.first().map(|item| item.mime.clone()),
            )
        }
    };
    MessageRecord {
        id: id.to_string(),
        contact_id: contact_id.to_string(),
        direction: direction.into(),
        body,
        status: status.into(),
        created_at: Utc::now(),
        kind,
        attachment_name,
        attachment_mime,
        edited_at: None,
        deleted_at: None,
        revision: 0,
        hidden_locally: false,
    }
}

pub fn save_payload_attachments(data_dir: &Path, msg_id: &str, payload: &ChatPayload) -> Result<()> {
    match payload {
        ChatPayload::Text { .. } => Ok(()),
        ChatPayload::File {
            name,
            mime,
            data,
            keep_attachment,
            ..
        } => {
            if *keep_attachment || data.is_empty() {
                return Ok(());
            }
            write_attachment_files(
                data_dir,
                msg_id,
                &[StoredMeta {
                    name: name.clone(),
                    mime: mime.clone(),
                }],
                &[data.as_slice()],
            )
        }
        ChatPayload::Album { items, .. } => {
            let metas = items
                .iter()
                .map(|item| StoredMeta {
                    name: item.name.clone(),
                    mime: item.mime.clone(),
                })
                .collect::<Vec<_>>();
            let blobs = items.iter().map(|item| item.data.as_slice()).collect::<Vec<_>>();
            write_attachment_files(data_dir, msg_id, &metas, &blobs)
        }
    }
}

pub fn save_payload_attachments_if_needed(
    data_dir: &Path,
    msg_id: &str,
    payload: &ChatPayload,
) -> Result<()> {
    save_payload_attachments(data_dir, msg_id, payload)
}

pub fn media_caption_from_record(record: &MessageRecord) -> Option<String> {
    let name = record.attachment_name.as_deref().unwrap_or("");
    if record.body.is_empty() || record.body == name {
        None
    } else {
        Some(record.body.clone())
    }
}

pub fn media_edit_payload(record: &MessageRecord, caption: Option<&str>) -> Result<ChatPayload> {
    match record.kind.as_str() {
        "image" | "file" => {
            let caption = caption
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Ok(ChatPayload::File {
                name: record
                    .attachment_name
                    .clone()
                    .unwrap_or_else(|| "file".into()),
                mime: record
                    .attachment_mime
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".into()),
                data: Vec::new(),
                caption,
                keep_attachment: true,
            })
        }
        _ => bail!("only media messages support caption edits"),
    }
}

fn write_attachment_files(
    data_dir: &Path,
    msg_id: &str,
    metas: &[StoredMeta],
    blobs: &[&[u8]],
) -> Result<()> {
    let dir = attachments_dir(data_dir, msg_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).ok();
    }
    std::fs::create_dir_all(&dir)?;
    for (index, blob) in blobs.iter().enumerate() {
        std::fs::write(dir.join(index.to_string()), blob)?;
    }
    let meta_path = meta_path(data_dir, msg_id);
    std::fs::write(meta_path, serde_json::to_vec(metas)?)?;
    Ok(())
}

pub fn read_attachment_bytes(data_dir: &Path, msg_id: &str, index: usize) -> Result<AttachmentData> {
    let metas = read_attachment_meta(data_dir, msg_id);
    let meta = metas
        .get(index)
        .with_context(|| format!("attachment index {index} missing"))?;
    let bytes = std::fs::read(attachments_dir(data_dir, msg_id).join(index.to_string()))?;
    Ok(AttachmentData {
        name: meta.name.clone(),
        mime: meta.mime.clone(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

pub fn attachment_count(data_dir: &Path, msg_id: &str) -> usize {
    read_attachment_meta(data_dir, msg_id).len()
}

fn read_attachment_meta(data_dir: &Path, msg_id: &str) -> Vec<StoredMeta> {
    let path = meta_path(data_dir, msg_id);
    if let Ok(bytes) = std::fs::read(path) {
        if let Ok(metas) = serde_json::from_slice::<Vec<StoredMeta>>(&bytes) {
            return metas;
        }
    }
    Vec::new()
}

fn attachments_dir(data_dir: &Path, msg_id: &str) -> PathBuf {
    data_dir.join("attachments").join(msg_id)
}

fn meta_path(data_dir: &Path, msg_id: &str) -> PathBuf {
    data_dir.join("attachments").join(format!("{msg_id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_payload_roundtrip() {
        let payload = ChatPayload::Text {
            body: "hello".into(),
        };
        let bytes = payload_to_bytes(&payload).unwrap();
        let decoded = bytes_to_payload(&bytes);
        match decoded {
            ChatPayload::Text { body } => assert_eq!(body, "hello"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn legacy_plaintext_fallback() {
        let decoded = bytes_to_payload(b"legacy text");
        match decoded {
            ChatPayload::Text { body } => assert_eq!(body, "legacy text"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn media_prefers_mailbox() {
        let payload = ChatPayload::File {
            name: "x.gif".into(),
            mime: "image/gif".into(),
            data: vec![1, 2, 3],
            caption: Some("hi".into()),
            keep_attachment: false,
        };
        assert!(prefers_mailbox_delivery(&payload, 10));
        let text = ChatPayload::Text {
            body: "short".into(),
        };
        assert!(!prefers_mailbox_delivery(&text, 10));
        let long = ChatPayload::Text {
            body: "x".repeat(50_000),
        };
        assert!(prefers_mailbox_delivery(&long, 50_000));
    }
}
