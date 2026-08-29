use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("decryption failed")]
    DecryptionFailed,
    #[error("invalid key material")]
    InvalidKey,
    #[error("invalid message")]
    InvalidMessage,
    #[error("signature verification failed")]
    InvalidSignature,
}
