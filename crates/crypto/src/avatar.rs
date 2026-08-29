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

const AVATAR_INFO: &[u8] = b"corgigram/avatar-v1";

/// Pairwise E2E avatar blob — only the recipient (viewer) can decrypt.
pub fn encrypt_avatar(local: &Identity, viewer: &PreKeyBundle, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let key = derive_avatar_key(local, viewer)?;
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

pub fn decrypt_avatar(local: &Identity, owner: &PreKeyBundle, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if blob.len() < 12 {
        return Err(CryptoError::InvalidMessage);
    }
    let key = derive_avatar_key(local, owner)?;
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

fn derive_avatar_key(local: &Identity, remote: &PreKeyBundle) -> Result<[u8; 32], CryptoError> {
    let shared = derive_shared_secret(
        local.agreement_secret(),
        &remote.identity.agreement_key(),
    );
    let hkdf = Hkdf::<Sha256>::new(None, &shared);
    let mut key = [0u8; 32];
    hkdf.expand(AVATAR_INFO, &mut key)
        .map_err(|_| CryptoError::InvalidKey)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_roundtrip() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");
        let png = b"\x89PNG\r\n\x1a\nfake";
        let enc = encrypt_avatar(&alice, &bob.prekey_bundle(), png).unwrap();
        let dec = decrypt_avatar(&bob, &alice.prekey_bundle(), &enc).unwrap();
        assert_eq!(dec, png);
    }
}
