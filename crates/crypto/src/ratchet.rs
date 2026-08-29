use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret};

use crate::CryptoError;

const MESSAGE_KEY_INFO: &[u8] = b"corgigram/message-key";
const CHAIN_KEY_INFO: &[u8] = b"corgigram/chain-key";
const ROOT_KEY_INFO: &[u8] = b"corgigram/root-key";

#[derive(Clone)]
pub struct RatchetSession {
    root_key: [u8; 32],
    send_chain_key: [u8; 32],
    recv_chain_key: [u8; 32],
    send_counter: u64,
    recv_counter: u64,
}

impl RatchetSession {
    pub fn initiator(shared_secret: [u8; 32]) -> Self {
        let (root_key, send_chain_key, recv_chain_key) = derive_session_keys(&shared_secret);
        Self {
            root_key,
            send_chain_key,
            recv_chain_key,
            send_counter: 0,
            recv_counter: 0,
        }
    }

    pub fn responder(shared_secret: [u8; 32]) -> Self {
        let (root_key, send_chain_key, recv_chain_key) = derive_session_keys(&shared_secret);
        Self {
            root_key,
            send_chain_key: recv_chain_key,
            recv_chain_key: send_chain_key,
            send_counter: 0,
            recv_counter: 0,
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let (message_key, next_chain_key) = derive_message_key(&self.send_chain_key);
        self.send_chain_key = next_chain_key;

        let counter = self.send_counter;
        self.send_counter += 1;

        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..].copy_from_slice(&counter.to_be_bytes());

        let cipher = ChaCha20Poly1305::new_from_slice(&message_key).map_err(|_| CryptoError::InvalidKey)?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: &counter.to_be_bytes(),
                },
            )
            .map_err(|_| CryptoError::DecryptionFailed)?;

        let mut output = Vec::with_capacity(8 + ciphertext.len());
        output.extend_from_slice(&counter.to_be_bytes());
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if ciphertext.len() < 8 {
            return Err(CryptoError::InvalidMessage);
        }

        let counter = u64::from_be_bytes(ciphertext[..8].try_into().unwrap());
        if counter != self.recv_counter {
            return Err(CryptoError::InvalidMessage);
        }

        let (message_key, next_chain_key) = derive_message_key(&self.recv_chain_key);
        self.recv_chain_key = next_chain_key;
        self.recv_counter += 1;

        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..].copy_from_slice(&counter.to_be_bytes());

        let cipher = ChaCha20Poly1305::new_from_slice(&message_key).map_err(|_| CryptoError::InvalidKey)?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &ciphertext[8..],
                    aad: &counter.to_be_bytes(),
                },
            )
            .map_err(|_| CryptoError::DecryptionFailed)
    }

    pub fn ratchet_step(&mut self, remote_public: &X25519Public, local_secret: &StaticSecret) {
        let shared = local_secret.diffie_hellman(remote_public);
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);

        let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
        hkdf.expand(ROOT_KEY_INFO, &mut self.root_key)
            .expect("hkdf expand root key");

        let hkdf = Hkdf::<Sha256>::new(Some(&self.root_key), b"ratchet");
        let mut send_chain = [0u8; 32];
        let mut recv_chain = [0u8; 32];
        hkdf.expand(b"send", &mut send_chain).expect("hkdf send");
        hkdf.expand(b"recv", &mut recv_chain).expect("hkdf recv");

        self.send_chain_key = send_chain;
        self.recv_chain_key = recv_chain;
        self.send_counter = 0;
        self.recv_counter = 0;
    }
}

pub fn derive_shared_secret(
    local_secret: &StaticSecret,
    remote_public: &X25519Public,
) -> [u8; 32] {
    let raw = local_secret.diffie_hellman(remote_public);
    let hkdf = Hkdf::<Sha256>::new(None, raw.as_bytes());
    let mut okm = [0u8; 32];
    hkdf.expand(b"corgigram/session", &mut okm)
        .expect("hkdf expand session");
    okm
}

fn derive_session_keys(shared_secret: &[u8; 32]) -> ([u8; 32], [u8; 32], [u8; 32]) {
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut root_key = [0u8; 32];
    let mut send_chain = [0u8; 32];
    let mut recv_chain = [0u8; 32];
    hkdf.expand(ROOT_KEY_INFO, &mut root_key).expect("root");
    hkdf.expand(b"send-chain", &mut send_chain).expect("send");
    hkdf.expand(b"recv-chain", &mut recv_chain).expect("recv");
    (root_key, send_chain, recv_chain)
}

fn derive_message_key(chain_key: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let hkdf = Hkdf::<Sha256>::new(None, chain_key);
    let mut message_key = [0u8; 32];
    let mut next_chain_key = [0u8; 32];
    hkdf.expand(MESSAGE_KEY_INFO, &mut message_key)
        .expect("message key");
    hkdf.expand(CHAIN_KEY_INFO, &mut next_chain_key)
        .expect("chain key");
    (message_key, next_chain_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    #[test]
    fn initiator_and_responder_can_exchange_messages() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");

        let shared_alice = derive_shared_secret(
            alice.agreement_secret(),
            &bob.public.agreement_key(),
        );
        let shared_bob = derive_shared_secret(
            bob.agreement_secret(),
            &alice.public.agreement_key(),
        );

        let mut alice_session = RatchetSession::initiator(shared_alice);
        let mut bob_session = RatchetSession::responder(shared_bob);

        let encrypted = alice_session.encrypt(b"hello bob").unwrap();
        let decrypted = bob_session.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"hello bob");

        let reply = bob_session.encrypt(b"hello alice").unwrap();
        let decrypted_reply = alice_session.decrypt(&reply).unwrap();
        assert_eq!(decrypted_reply, b"hello alice");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");

        let shared_alice = derive_shared_secret(
            alice.agreement_secret(),
            &bob.public.agreement_key(),
        );
        let shared_bob = derive_shared_secret(
            bob.agreement_secret(),
            &alice.public.agreement_key(),
        );

        let mut alice_session = RatchetSession::initiator(shared_alice);
        let mut bob_session = RatchetSession::responder(shared_bob);

        let mut encrypted = alice_session.encrypt(b"secret").unwrap();
        if let Some(byte) = encrypted.last_mut() {
            *byte ^= 0x01;
        }
        assert!(bob_session.decrypt(&encrypted).is_err());
    }
}
