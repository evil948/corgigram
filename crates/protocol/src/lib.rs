use serde::{Deserialize, Serialize};

use corgigram_crypto::{SessionAckMessage, SessionInitMessage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    SessionInit(SessionInitMessage),
    SessionAck(SessionAckMessage),
    EncryptedChat { ciphertext: Vec<u8> },
    Ping,
    Pong,
}

impl WireMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        serde_json::to_vec(self).map_err(|_| ProtocolError::Encode)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        serde_json::from_slice(bytes).map_err(|_| ProtocolError::Decode)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("failed to encode message")]
    Encode,
    #[error("failed to decode message")]
    Decode,
}
