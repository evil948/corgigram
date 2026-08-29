use serde::{Deserialize, Serialize};

use crate::{
    identity::{Identity, PreKeyBundle, PublicIdentity},
    ratchet::{derive_shared_secret, RatchetSession},
    CryptoError,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInitMessage {
    pub sender: PublicIdentity,
    pub prekey: PreKeyBundle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionAckMessage {
    pub sender: PublicIdentity,
    pub prekey: PreKeyBundle,
}

pub struct SessionInitiator {
    identity: Identity,
    session: RatchetSession,
    remote: PublicIdentity,
}

pub struct SessionResponder {
    identity: Identity,
    session: RatchetSession,
    remote: PublicIdentity,
}

pub struct Session {
    identity: Identity,
    session: RatchetSession,
    remote: PublicIdentity,
}

impl SessionInitiator {
    pub fn begin(identity: Identity, remote_bundle: &PreKeyBundle) -> Result<(Self, SessionInitMessage), CryptoError> {
        remote_bundle.verify()?;
        let shared = derive_shared_secret(
            identity.agreement_secret(),
            &remote_bundle.identity.agreement_key(),
        );

        let init = SessionInitMessage {
            sender: identity.public.clone(),
            prekey: identity.prekey_bundle(),
        };

        Ok((
            Self {
                identity,
                session: RatchetSession::initiator(shared),
                remote: remote_bundle.identity.clone(),
            },
            init,
        ))
    }

    pub fn complete(self, ack: &SessionAckMessage) -> Result<Session, CryptoError> {
        ack.prekey.verify()?;
        if ack.sender.user_id != self.remote.user_id {
            return Err(CryptoError::InvalidMessage);
        }

        Ok(Session {
            identity: self.identity,
            session: self.session,
            remote: self.remote,
        })
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.session.encrypt(plaintext)
    }
}

impl SessionResponder {
    pub fn accept(identity: Identity, init: &SessionInitMessage) -> Result<(Self, SessionAckMessage), CryptoError> {
        init.prekey.verify()?;
        let shared = derive_shared_secret(
            identity.agreement_secret(),
            &init.sender.agreement_key(),
        );

        let ack = SessionAckMessage {
            sender: identity.public.clone(),
            prekey: identity.prekey_bundle(),
        };

        Ok((
            Self {
                identity,
                session: RatchetSession::responder(shared),
                remote: init.sender.clone(),
            },
            ack,
        ))
    }

    pub fn complete(self, _init: &SessionInitMessage) -> Result<Session, CryptoError> {
        Ok(Session {
            identity: self.identity,
            session: self.session,
            remote: self.remote,
        })
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.session.encrypt(plaintext)
    }
}

impl Session {
    pub fn remote(&self) -> &PublicIdentity {
        &self.remote
    }

    pub fn local(&self) -> &PublicIdentity {
        &self.identity.public
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.session.encrypt(plaintext)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.session.decrypt(ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_session_handshake() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");
        let bob_bundle = bob.prekey_bundle();

        let (alice_initiator, init) = SessionInitiator::begin(alice, &bob_bundle).unwrap();
        let (bob_responder, ack) = SessionResponder::accept(bob, &init).unwrap();
        let mut alice_session = alice_initiator.complete(&ack).unwrap();
        let mut bob_session = bob_responder.complete(&init).unwrap();

        let encrypted = alice_session.encrypt(b"ping").unwrap();
        let decrypted = bob_session.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"ping");
    }
}
