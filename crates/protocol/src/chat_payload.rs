use serde::{Deserialize, Serialize};

/// Plaintext chat envelope (serialized to JSON, then encrypted E2E).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum ChatPayload {
    #[serde(rename = "text")]
    Text { body: String },
    #[serde(rename = "file")]
    File {
        name: String,
        mime: String,
        /// Raw bytes (not base64) — serde_json encodes as byte array.
        data: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    #[serde(rename = "album")]
    Album {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        items: Vec<AttachmentItem>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentItem {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

impl ChatPayload {
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Legacy plain UTF-8 messages (pre-payload).
    pub fn from_legacy_plaintext(bytes: &[u8]) -> Self {
        Self::Text {
            body: String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_payload_json_roundtrip() {
        let payload = ChatPayload::File {
            name: "photo.png".into(),
            mime: "image/png".into(),
            data: vec![1, 2, 3],
            caption: Some("look".into()),
        };
        let bytes = payload.to_bytes().unwrap();
        let decoded = ChatPayload::from_bytes(&bytes).unwrap();
        assert_eq!(payload, decoded);
    }
}
