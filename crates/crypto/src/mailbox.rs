use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

use crate::identity::{Identity, PreKeyBundle};
use crate::ratchet::derive_shared_secret;
use crate::CryptoError;

const MAILBOX_INFO: &[u8] = b"corgigram/mailbox-v1";

/// Offline mailbox encryption (pairwise static key derived from long-term agreement keys).
pub fn encrypt_mailbox(local: &Identity, remote: &PreKeyBundle, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let key = derive_mailbox_key(local, remote)?;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);

    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| CryptoError::InvalidKey)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: b"" })
        .map_err(|_| CryptoError::DecryptionFailed)?;

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_mailbox(local: &Identity, remote: &PreKeyBundle, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if blob.len() < 12 {
        return Err(CryptoError::InvalidMessage);
    }
    let key = derive_mailbox_key(local, remote)?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| CryptoError::InvalidKey)?;
    cipher
        .decrypt(
            Nonce::from_slice(&blob[..12]),
            Payload {
                msg: &blob[12..],
                aad: b"",
            },
        )
        .map_err(|_| CryptoError::DecryptionFailed)
}

fn derive_mailbox_key(local: &Identity, remote: &PreKeyBundle) -> Result<[u8; 32], CryptoError> {
    let shared = derive_shared_secret(
        local.agreement_secret(),
        &remote.identity.agreement_key(),
    );
    let hkdf = Hkdf::<Sha256>::new(None, &shared);
    let mut key = [0u8; 32];
    hkdf.expand(MAILBOX_INFO, &mut key)
        .map_err(|_| CryptoError::InvalidKey)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_roundtrip() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");
        let encrypted = encrypt_mailbox(&alice, &bob.prekey_bundle(), b"offline hello").unwrap();
        let decrypted = decrypt_mailbox(&bob, &alice.prekey_bundle(), &encrypted).unwrap();
        assert_eq!(decrypted, b"offline hello");
    }
}
