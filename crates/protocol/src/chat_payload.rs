use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
        #[serde(with = "serde_bytes_b64")]
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
    #[serde(with = "serde_bytes_b64")]
    pub data: Vec<u8>,
}

mod serde_bytes_b64 {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(serde::de::Error::custom),
            serde_json::Value::Array(items) => items
                .into_iter()
                .map(|item| {
                    item.as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or_else(|| serde::de::Error::custom("invalid byte in attachment data"))
                })
                .collect(),
            _ => Err(serde::de::Error::custom(
                "attachment data must be base64 string or byte array",
            )),
        }
    }
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
        let json = std::str::from_utf8(&bytes).unwrap();
        assert!(json.contains("\"data\":\"AQID\""));
        let decoded = ChatPayload::from_bytes(&bytes).unwrap();
        assert_eq!(payload, decoded);
    }

    #[test]
    fn legacy_byte_array_payload_still_parses() {
        let json = r#"{"kind":"file","name":"x.gif","mime":"image/gif","data":[1,2,3]}"#;
        let decoded = ChatPayload::from_bytes(json.as_bytes()).unwrap();
        match decoded {
            ChatPayload::File { data, .. } => assert_eq!(data, vec![1, 2, 3]),
            _ => panic!("expected file"),
        }
    }
}
