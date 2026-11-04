//! Webhook authentication helpers: constant-time comparison and GitHub-style
//! `X-Hub-Signature-256: sha256=<hex HMAC-SHA256(secret, body)>` verification.

use sha2::{Digest, Sha256};

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.iter().map(|b| b ^ 0x36).collect::<Vec<_>>());
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.iter().map(|b| b ^ 0x5c).collect::<Vec<_>>());
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Accepts `sha256=<hex>` (GitHub, Gitea…) or a bare hex digest.
pub fn verify_signature(secret: &[u8], body: &[u8], signature: &str) -> bool {
    let given = signature.trim().trim_start_matches("sha256=").to_ascii_lowercase();
    constant_time_eq(given.as_bytes(), hex(&hmac_sha256(secret, body)).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231_and_github_format() {
        // RFC 4231 test case 2.
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        let sig = format!("sha256={}", hex(&hmac_sha256(b"s3cret", b"{\"a\":1}")));
        assert!(verify_signature(b"s3cret", b"{\"a\":1}", &sig));
        assert!(!verify_signature(b"altro", b"{\"a\":1}", &sig));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
