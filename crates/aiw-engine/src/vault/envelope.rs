//! The sealed form of a secret string: AES-256-GCM with a random nonce, no new dependency (`ring` is already the TLS stack).
//!
//! `aiwenc1:<key id>:<base64url(nonce ‖ ciphertext ‖ tag)>`. The key id is the first four bytes of a SHA-256 of the key (it
//! says *which* key sealed the value, so a value sealed by another key is recognised without trying to open it); it reveals
//! nothing about the key. The authenticated data is a fixed label: the value is not bound to the document that holds it, so
//! that moving or renaming a document (a migration of the database, a restore) never makes its secrets unreadable.

use aiw_core::sealed::SEALED_PREFIX;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};

/// Bytes of the key.
pub const KEY_LEN: usize = 32;
const AAD: &[u8] = b"aiw-secret-v1";

/// The id of a key (see the module documentation).
pub fn key_id(key: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"aiw-key-id-v1");
    h.update(key);
    h.finalize()[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// `n` random bytes from the system generator.
pub fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; n];
    SystemRandom::new().fill(&mut out).map_err(|_| "il generatore di numeri casuali del sistema non risponde".to_string())?;
    Ok(out)
}

/// A key ready to seal and open.
pub struct Key {
    aead: LessSafeKey,
    id: String,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({})", self.id)
    }
}

impl Key {
    /// A key from its bytes (exactly [`KEY_LEN`]).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != KEY_LEN {
            return Err(format!("la chiave ha {} byte invece di {KEY_LEN}", bytes.len()));
        }
        let unbound = UnboundKey::new(&AES_256_GCM, bytes).map_err(|_| "chiave non valida".to_string())?;
        Ok(Self { aead: LessSafeKey::new(unbound), id: key_id(bytes) })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The sealed form of `plain`. A new random nonce every time: sealing the same value twice gives two different strings.
    pub fn seal(&self, plain: &str) -> Result<String, String> {
        let nonce_bytes = random_bytes(NONCE_LEN)?;
        let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes).map_err(|_| "nonce non valido".to_string())?;
        let mut blob = plain.as_bytes().to_vec();
        self.aead.seal_in_place_append_tag(nonce, Aad::from(AAD), &mut blob).map_err(|_| "cifratura non riuscita".to_string())?;
        let mut out = nonce_bytes;
        out.extend_from_slice(&blob);
        Ok(format!("{SEALED_PREFIX}{}:{}", self.id, URL_SAFE_NO_PAD.encode(out)))
    }

    /// The value inside `stored`, or `None` when it is not sealed with this key (another key id, a newer format, a damaged or
    /// altered string).
    pub fn open(&self, stored: &str) -> Option<String> {
        let (id, blob) = parse(stored)?;
        if id != self.id || blob.len() < NONCE_LEN + AES_256_GCM.tag_len() {
            return None;
        }
        let (nonce_bytes, rest) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce_bytes).ok()?;
        let mut buf = rest.to_vec();
        let plain = self.aead.open_in_place(nonce, Aad::from(AAD), &mut buf).ok()?;
        String::from_utf8(plain.to_vec()).ok()
    }
}

/// The key id and the raw bytes of a sealed string, or `None` when `stored` is not one.
pub fn parse(stored: &str) -> Option<(&str, Vec<u8>)> {
    let rest = stored.strip_prefix(SEALED_PREFIX)?;
    let (id, b64) = rest.split_once(':')?;
    Some((id, URL_SAFE_NO_PAD.decode(b64).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        Key::from_bytes(&[7u8; KEY_LEN]).unwrap()
    }

    #[test]
    fn a_value_round_trips_and_the_envelope_hides_it() {
        let k = key();
        for plain in ["hf_TEST_ONLY_not_a_real_token", "", "àèì — «segreto» 🔑", &"x".repeat(5000)] {
            let sealed = k.seal(plain).unwrap();
            assert!(aiw_core::sealed::is_sealed(&sealed));
            assert!(plain.len() < 4 || !sealed.contains(plain), "the sealed string must not hold the plain one");
            assert_eq!(k.open(&sealed).as_deref(), Some(plain));
        }
    }

    #[test]
    fn sealing_twice_gives_two_strings_and_both_open() {
        let k = key();
        let (a, b) = (k.seal("same").unwrap(), k.seal("same").unwrap());
        assert_ne!(a, b, "the nonce is fresh every time");
        assert_eq!((k.open(&a).as_deref(), k.open(&b).as_deref()), (Some("same"), Some("same")));
    }

    #[test]
    fn another_key_a_damaged_string_and_a_newer_format_do_not_open() {
        let (k, other) = (key(), Key::from_bytes(&[9u8; KEY_LEN]).unwrap());
        let sealed = k.seal("segreto").unwrap();
        assert_ne!(k.id(), other.id());
        assert_eq!(other.open(&sealed), None);
        // One flipped character in the ciphertext: the tag no longer matches.
        let mut chars: Vec<char> = sealed.chars().collect();
        let last = chars.len() - 3;
        chars[last] = if chars[last] == 'A' { 'B' } else { 'A' };
        assert_eq!(k.open(&chars.into_iter().collect::<String>()), None);
        // Cut short, not base64, wrong version, not an envelope at all.
        assert_eq!(k.open(&sealed[..sealed.len() - 30]), None);
        assert_eq!(k.open(&format!("aiwenc1:{}:!!!", k.id())), None);
        assert_eq!(k.open(&sealed.replacen("aiwenc1:", "aiwenc2:", 1)), None);
        assert_eq!(k.open("hf_plain_token"), None);
    }

    #[test]
    fn a_key_of_the_wrong_length_is_refused_and_ids_are_stable() {
        assert!(Key::from_bytes(&[1u8; 16]).is_err());
        assert_eq!(key_id(&[7u8; KEY_LEN]), key().id());
        assert_ne!(key_id(&[7u8; KEY_LEN]), key_id(&[8u8; KEY_LEN]));
        assert_eq!(random_bytes(KEY_LEN).unwrap().len(), KEY_LEN);
    }
}
