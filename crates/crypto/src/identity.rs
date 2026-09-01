use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519Public, StaticSecret};

use crate::CryptoError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub user_id: String,
    pub display_name: String,
    pub signing_key: [u8; 32],
    pub agreement_key: [u8; 32],
}

impl PublicIdentity {
    pub fn verifying_key(&self) -> Result<VerifyingKey, CryptoError> {
        VerifyingKey::from_bytes(&self.signing_key).map_err(|_| CryptoError::InvalidKey)
    }

    pub fn agreement_key(&self) -> X25519Public {
        X25519Public::from(self.agreement_key)
    }

    pub fn safety_number(&self, other: &PublicIdentity) -> String {
        let (first, second) = if self.user_id <= other.user_id {
            (self, other)
        } else {
            (other, self)
        };
        let mut hasher = Sha256::new();
        hasher.update(first.user_id.as_bytes());
        hasher.update(&first.signing_key);
        hasher.update(&first.agreement_key);
        hasher.update(second.user_id.as_bytes());
        hasher.update(&second.signing_key);
        hasher.update(&second.agreement_key);
        let digest = hasher.finalize();
        format_safety_number(&digest)
    }
}

#[derive(Serialize, Deserialize)]
struct IdentityStored {
    public: PublicIdentity,
    signing_key: [u8; 32],
    agreement_secret: [u8; 32],
}

pub struct Identity {
    pub public: PublicIdentity,
    signing_key: SigningKey,
    agreement_secret: StaticSecret,
}

impl Clone for Identity {
    fn clone(&self) -> Self {
        Self {
            public: self.public.clone(),
            signing_key: self.signing_key.clone(),
            agreement_secret: StaticSecret::from(self.agreement_secret.to_bytes()),
        }
    }
}

impl Identity {
    pub fn generate(user_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let agreement_secret = StaticSecret::random_from_rng(&mut csprng);
        let agreement_public = X25519Public::from(&agreement_secret);

        Self {
            public: PublicIdentity {
                user_id: user_id.into(),
                display_name: display_name.into(),
                signing_key: signing_key.verifying_key().to_bytes(),
                agreement_key: agreement_public.to_bytes(),
            },
            signing_key,
            agreement_secret,
        }
    }

    pub fn to_stored(&self) -> IdentityStored {
        IdentityStored {
            public: self.public.clone(),
            signing_key: self.signing_key.to_bytes(),
            agreement_secret: self.agreement_secret.to_bytes(),
        }
    }

    pub fn from_stored(stored: IdentityStored) -> Result<Self, CryptoError> {
        let signing_key = SigningKey::from_bytes(&stored.signing_key);
        let agreement_secret = StaticSecret::from(stored.agreement_secret);
        Ok(Self {
            public: stored.public,
            signing_key,
            agreement_secret,
        })
    }

    pub fn save_json(&self) -> Result<String, CryptoError> {
        serde_json::to_string_pretty(&self.to_stored()).map_err(|_| CryptoError::InvalidKey)
    }

    pub fn load_json(json: &str) -> Result<Self, CryptoError> {
        let stored: IdentityStored = serde_json::from_str(json).map_err(|_| CryptoError::InvalidKey)?;
        Self::from_stored(stored)
    }

    pub fn prekey_bundle(&self) -> PreKeyBundle {
        PreKeyBundle {
            identity: self.public.clone(),
            signature: self.sign(&self.public.agreement_key).to_bytes().to_vec(),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    pub fn agreement_secret(&self) -> &StaticSecret {
        &self.agreement_secret
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreKeyBundle {
    pub identity: PublicIdentity,
    pub signature: Vec<u8>,
}

impl PreKeyBundle {
    pub fn verify(&self) -> Result<(), CryptoError> {
        let verifying_key = self.identity.verifying_key()?;
        let signature = Signature::from_slice(&self.signature).map_err(|_| CryptoError::InvalidSignature)?;
        verifying_key
            .verify(&self.identity.agreement_key, &signature)
            .map_err(|_| CryptoError::InvalidSignature)
    }
}

fn format_safety_number(digest: &[u8]) -> String {
    digest
        .iter()
        .take(15)
        .map(|byte| format!("{:02x}", byte))
        .collect::<Vec<_>>()
        .chunks(5)
        .map(|chunk| chunk.join(""))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prekey_bundle_verifies() {
        let alice = Identity::generate("alice", "Alice");
        let bundle = alice.prekey_bundle();
        assert!(bundle.verify().is_ok());
    }

    #[test]
    fn identity_roundtrip_json() {
        let alice = Identity::generate("alice", "Alice");
        let json = alice.save_json().unwrap();
        let loaded = Identity::load_json(&json).unwrap();
        assert_eq!(loaded.public.user_id, "alice");
        assert!(loaded.prekey_bundle().verify().is_ok());
    }

    #[test]
    fn safety_number_is_stable() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");
        let first = alice.public.safety_number(&bob.public);
        let second = alice.public.safety_number(&bob.public);
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    #[test]
    fn safety_number_is_symmetric_between_peers() {
        let alice = Identity::generate("alice", "Alice");
        let bob = Identity::generate("bob", "Bob");
        assert_eq!(
            alice.public.safety_number(&bob.public),
            bob.public.safety_number(&alice.public)
        );
    }
}
