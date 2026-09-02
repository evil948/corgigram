use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ChatPayload;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageOp {
    Create,
    Edit,
    Delete,
}

/// Wire plaintext envelope (JSON → E2E encrypt).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatEnvelope {
    #[serde(default = "default_version")]
    pub v: u8,
    /// Unique event id (for create: also the message id).
    pub id: String,
    pub op: MessageOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub ts: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ChatPayload>,
}

fn default_version() -> u8 {
    2
}

impl ChatEnvelope {
    pub fn create(id: impl Into<String>, payload: ChatPayload) -> Self {
        Self {
            v: 2,
            id: id.into(),
            op: MessageOp::Create,
            target_id: None,
            ts: Utc::now(),
            payload: Some(payload),
        }
    }

    pub fn edit(
        event_id: impl Into<String>,
        target_id: impl Into<String>,
        payload: ChatPayload,
    ) -> Self {
        Self {
            v: 2,
            id: event_id.into(),
            op: MessageOp::Edit,
            target_id: Some(target_id.into()),
            ts: Utc::now(),
            payload: Some(payload),
        }
    }

    pub fn delete(event_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            v: 2,
            id: event_id.into(),
            op: MessageOp::Delete,
            target_id: Some(target_id.into()),
            ts: Utc::now(),
            payload: None,
        }
    }

    pub fn message_id(&self) -> &str {
        match self.op {
            MessageOp::Create => &self.id,
            MessageOp::Edit | MessageOp::Delete => {
                self.target_id.as_deref().unwrap_or(&self.id)
            }
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Legacy v1: raw ChatPayload or UTF-8 text.
    pub fn from_legacy(bytes: &[u8], fallback_id: &str) -> Self {
        if let Ok(envelope) = Self::from_bytes(bytes) {
            if envelope.v >= 2 {
                return envelope;
            }
        }
        let payload = ChatPayload::from_bytes(bytes)
            .unwrap_or_else(|_| ChatPayload::from_legacy_plaintext(bytes));
        Self::create(fallback_id, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let env = ChatEnvelope::create(
            "msg-1",
            ChatPayload::Text {
                body: "hi".into(),
            },
        );
        let bytes = env.to_bytes().unwrap();
        let decoded = ChatEnvelope::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, env);
    }

    #[test]
    fn legacy_plaintext_wraps_as_create() {
        let env = ChatEnvelope::from_legacy(b"hello", "gen-id");
        assert_eq!(env.op, MessageOp::Create);
        assert_eq!(env.id, "gen-id");
        match env.payload.unwrap() {
            ChatPayload::Text { body } => assert_eq!(body, "hello"),
            _ => panic!("expected text"),
        }
    }
}
