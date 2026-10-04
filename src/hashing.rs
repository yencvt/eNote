use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

/// SHA/MD5 variants offered from the editor's "Hash" context menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    pub fn label(self) -> &'static str {
        match self {
            HashAlgorithm::Md5 => "MD5",
            HashAlgorithm::Sha1 => "SHA-1",
            HashAlgorithm::Sha256 => "SHA-256",
            HashAlgorithm::Sha384 => "SHA-384",
            HashAlgorithm::Sha512 => "SHA-512",
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Computes `algorithm` over `source`. When `key` is empty this is the plain digest;
/// otherwise it's the keyed HMAC variant, letting the key dialog double as a plain hash
/// (blank key) or an HMAC (non-blank key) tool.
pub fn compute_hash(algorithm: HashAlgorithm, source: &str, key: &str) -> String {
    let data = source.as_bytes();
    if key.is_empty() {
        return match algorithm {
            HashAlgorithm::Md5 => to_hex(&Md5::digest(data)),
            HashAlgorithm::Sha1 => to_hex(&Sha1::digest(data)),
            HashAlgorithm::Sha256 => to_hex(&Sha256::digest(data)),
            HashAlgorithm::Sha384 => to_hex(&Sha384::digest(data)),
            HashAlgorithm::Sha512 => to_hex(&Sha512::digest(data)),
        };
    }

    // HMAC accepts keys of any length (longer keys get hashed down internally), so these
    // `new_from_slice` calls cannot fail in practice.
    let key_bytes = key.as_bytes();
    match algorithm {
        HashAlgorithm::Md5 => {
            let mut mac =
                Hmac::<Md5>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(data);
            to_hex(&mac.finalize().into_bytes())
        }
        HashAlgorithm::Sha1 => {
            let mut mac =
                Hmac::<Sha1>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(data);
            to_hex(&mac.finalize().into_bytes())
        }
        HashAlgorithm::Sha256 => {
            let mut mac =
                Hmac::<Sha256>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(data);
            to_hex(&mac.finalize().into_bytes())
        }
        HashAlgorithm::Sha384 => {
            let mut mac =
                Hmac::<Sha384>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(data);
            to_hex(&mac.finalize().into_bytes())
        }
        HashAlgorithm::Sha512 => {
            let mut mac =
                Hmac::<Sha512>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(data);
            to_hex(&mac.finalize().into_bytes())
        }
    }
}

/// Default bcrypt cost (re-exported so the UI doesn't need to depend on the `bcrypt`
/// crate directly).
pub const DEFAULT_BCRYPT_COST: u32 = bcrypt::DEFAULT_COST;

/// Hashes `password` with bcrypt at the given cost (4-31; higher is slower/stronger).
/// Every call produces a different (but equally valid) hash, since bcrypt generates a
/// fresh random salt each time.
pub fn bcrypt_hash(password: &str, cost: u32) -> Result<String, String> {
    bcrypt::hash(password, cost).map_err(|e| format!("Bcrypt hashing failed: {e}"))
}

/// Checks whether `password` matches a bcrypt `hash` string (e.g. `$2b$12$...`).
pub fn bcrypt_verify(password: &str, hash: &str) -> Result<bool, String> {
    bcrypt::verify(password, hash.trim()).map_err(|e| format!("Bcrypt verify failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_known_vector() {
        assert_eq!(
            compute_hash(HashAlgorithm::Md5, "hello world", ""),
            "5eb63bbbe01eeed093cb22bb8f5acdc3"
        );
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            compute_hash(HashAlgorithm::Sha256, "hello world", ""),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn hmac_variant_differs_from_plain_digest() {
        let plain = compute_hash(HashAlgorithm::Sha256, "hello world", "");
        let keyed = compute_hash(HashAlgorithm::Sha256, "hello world", "secret");
        assert_ne!(plain, keyed);
        assert_eq!(keyed.len(), plain.len());
    }

    #[test]
    fn bcrypt_hash_then_verify_round_trips() {
        let hash = bcrypt_hash("correct horse battery staple", 4).unwrap();
        assert!(hash.starts_with("$2"));
        assert_eq!(
            bcrypt_verify("correct horse battery staple", &hash),
            Ok(true)
        );
        assert_eq!(bcrypt_verify("wrong password", &hash), Ok(false));
    }

    #[test]
    fn bcrypt_verify_rejects_malformed_hash() {
        assert!(bcrypt_verify("whatever", "not-a-bcrypt-hash").is_err());
    }
}
