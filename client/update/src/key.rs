//! The release's Ed25519 keys: the public half every client verifies with, the private half the release signs with.

use data_encoding::BASE64;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use std::fmt;
use zeroize::Zeroizing;

/// An Ed25519 signature's length in bytes: what `latest.json.sig` holds, raw.
pub const SIGNATURE_BYTES: usize = 64;

/// The longest the base64 of a signing key may be: a PKCS#8 v2 document with its public key is 83 bytes.
const MAX_SIGNING_KEY_TEXT: usize = 256;

/// A channel's public key: 32 raw bytes, as its key file (`release.pub`, `dev.pub`) holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyingKey([u8; VerifyingKey::LEN]);

impl VerifyingKey {
    /// An Ed25519 public key's length in bytes.
    pub const LEN: usize = 32;

    /// The key `bytes` are.
    pub const fn new(bytes: [u8; VerifyingKey::LEN]) -> VerifyingKey {
        VerifyingKey(bytes)
    }

    /// The raw key, as a channel's key file holds it.
    pub fn bytes(&self) -> &[u8; VerifyingKey::LEN] {
        &self.0
    }

    /// Whether `signature` is this key's over exactly `message`.
    pub(crate) fn verifies(&self, message: &[u8], signature: &[u8]) -> bool {
        UnparsedPublicKey::new(&ED25519, &self.0).verify(message, signature).is_ok()
    }
}

/// The release's private key, which signs a manifest. It is never printed, and the text it was read from is zeroed.
pub struct SigningKey(Ed25519KeyPair);

/// Why a signing key was refused: not base64, or not an Ed25519 key in PKCS#8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigningKeyRefused;

impl fmt::Display for SigningKeyRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not the base64 of an Ed25519 private key in PKCS#8 DER")
    }
}

impl std::error::Error for SigningKeyRefused {}

impl SigningKey {
    /// The key whose PKCS#8 DER `text` is the base64 of, as `openssl genpkey -algorithm ed25519 | openssl pkey
    /// -outform DER | base64 -w0` writes it (PKCS#8 v1, which carries no public key) or with its public key (v2).
    pub fn from_base64(text: &str) -> Result<SigningKey, SigningKeyRefused> {
        let text = text.trim_ascii();
        if text.len() > MAX_SIGNING_KEY_TEXT {
            return Err(SigningKeyRefused);
        }
        let der = Zeroizing::new(BASE64.decode(text.as_bytes()).map_err(|_| SigningKeyRefused)?);
        // ring's `from_pkcs8` takes only v2, and OpenSSL writes v1; a v2 document's public key is still checked.
        Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der).map(SigningKey).map_err(|_| SigningKeyRefused)
    }

    /// The public half, which a client verifies with.
    pub fn verifying_key(&self) -> VerifyingKey {
        let mut bytes = [0u8; VerifyingKey::LEN];
        bytes.copy_from_slice(self.0.public_key().as_ref());
        VerifyingKey(bytes)
    }

    /// This key's signature over `message`.
    pub(crate) fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_BYTES] {
        let mut signature = [0u8; SIGNATURE_BYTES];
        signature.copy_from_slice(self.0.sign(message).as_ref());
        signature
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SigningKey([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;

    /// What OpenSSL writes before an Ed25519 seed in PKCS#8 v1 DER (`openssl pkey -outform DER`, 48 bytes).
    const OPENSSL_V1_PREFIX: [u8; 16] =
        [0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20];

    #[test]
    fn a_key_as_openssl_writes_it_is_taken() {
        let seed = [7u8; 32];
        let der = [OPENSSL_V1_PREFIX.as_slice(), &seed].concat();
        let key = SigningKey::from_base64(&format!("{}\n", BASE64.encode(&der))).unwrap();
        let expected = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        assert_eq!(key.verifying_key().bytes().as_slice(), expected.public_key().as_ref());
    }

    #[test]
    fn a_key_with_its_public_half_is_taken_and_signs_what_its_public_half_verifies() {
        let der = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = SigningKey::from_base64(&BASE64.encode(der.as_ref())).unwrap();
        let signature = key.sign(b"manifest");
        assert!(key.verifying_key().verifies(b"manifest", &signature));
        assert!(!key.verifying_key().verifies(b"manifesto", &signature));
        assert!(!key.verifying_key().verifies(b"manifest", &signature[..SIGNATURE_BYTES - 1]));
    }

    #[test]
    fn anything_but_a_key_is_refused_and_a_key_never_prints() {
        for text in ["", "not base64!", "AAAA", &"A".repeat(MAX_SIGNING_KEY_TEXT + 4)] {
            assert_eq!(SigningKey::from_base64(text).err(), Some(SigningKeyRefused), "{text:?}");
        }
        let der = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = SigningKey::from_base64(&BASE64.encode(der.as_ref())).unwrap();
        assert_eq!(format!("{key:?}"), "SigningKey([redacted])");
    }
}
