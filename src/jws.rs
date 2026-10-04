//! JWS (JSON Web Signature) generation - the "sign" counterpart to `jwt.rs`'s decoder.
//!
//! Builds a compact `header.payload.signature` token (RFC 7515) for any of the twelve
//! algorithms the JWS spec commonly supports, grouped into four families that differ in
//! what kind of key material they need:
//! - HMAC (HS256/384/512): a plain shared-secret string.
//! - RSA PKCS#1 v1.5 (RS256/384/512) and RSA-PSS (PS256/384/512): an RSA private key,
//!   PEM-encoded as either PKCS#1 (`BEGIN RSA PRIVATE KEY`) or PKCS#8 (`BEGIN PRIVATE KEY`).
//! - ECDSA (ES256/384/512): an EC private key on the matching curve (P-256/P-384/P-521),
//!   PEM-encoded as either SEC1 (`BEGIN EC PRIVATE KEY`) or PKCS#8.
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rsa::RsaPrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use sha2::{Sha256, Sha384, Sha512};

/// The signing algorithm to use for a JWS, mirroring the `alg` values from RFC 7518.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JwsAlgorithm {
    Hs256,
    Hs384,
    Hs512,
    Rs256,
    Rs384,
    Rs512,
    Ps256,
    Ps384,
    Ps512,
    Es256,
    Es384,
    Es512,
}

/// A family of related algorithms, used to group the algorithm picker and to decide what
/// kind of key material a given algorithm expects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JwsAlgorithmFamily {
    Hmac,
    RsaPkcs1,
    RsaPss,
    Ecdsa,
}

impl JwsAlgorithmFamily {
    pub fn label(self) -> &'static str {
        match self {
            JwsAlgorithmFamily::Hmac => "HMAC (Symmetric)",
            JwsAlgorithmFamily::RsaPkcs1 => "RSA PKCS#1",
            JwsAlgorithmFamily::RsaPss => "RSA PSS",
            JwsAlgorithmFamily::Ecdsa => "ECDSA (Elliptic Curve)",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            JwsAlgorithmFamily::Hmac => "Shared secret key - best for internal services",
            JwsAlgorithmFamily::RsaPkcs1 => "Public/private key pair - widely compatible",
            JwsAlgorithmFamily::RsaPss => "Modern RSA - recommended for new applications",
            JwsAlgorithmFamily::Ecdsa => "Smaller signatures - fast & modern",
        }
    }

    /// What kind of text the key box should contain for algorithms in this family.
    pub fn key_hint(self) -> &'static str {
        match self {
            JwsAlgorithmFamily::Hmac => "Shared secret (plain text, any length)",
            JwsAlgorithmFamily::RsaPkcs1 | JwsAlgorithmFamily::RsaPss => {
                "RSA private key - PEM, PKCS#1 \"BEGIN RSA PRIVATE KEY\" or PKCS#8 \"BEGIN PRIVATE KEY\""
            }
            JwsAlgorithmFamily::Ecdsa => {
                "EC private key - PEM, SEC1 \"BEGIN EC PRIVATE KEY\" or PKCS#8 \"BEGIN PRIVATE KEY\" (must match the curve below)"
            }
        }
    }

    /// Same idea as `key_hint`, but for the verify tool: asymmetric algorithms only need
    /// the *public* key (though a private key is also accepted, for convenience, and its
    /// public part will be used).
    pub fn verify_key_hint(self) -> &'static str {
        match self {
            JwsAlgorithmFamily::Hmac => "Shared secret (plain text, any length)",
            JwsAlgorithmFamily::RsaPkcs1 | JwsAlgorithmFamily::RsaPss => {
                "RSA public key - PEM, SPKI \"BEGIN PUBLIC KEY\" or PKCS#1 \"BEGIN RSA PUBLIC KEY\" (a private key also works)"
            }
            JwsAlgorithmFamily::Ecdsa => {
                "EC public key - PEM, SPKI \"BEGIN PUBLIC KEY\" (a private key also works)"
            }
        }
    }
}

impl JwsAlgorithm {
    pub const ALL: [JwsAlgorithm; 12] = [
        JwsAlgorithm::Hs256,
        JwsAlgorithm::Hs384,
        JwsAlgorithm::Hs512,
        JwsAlgorithm::Rs256,
        JwsAlgorithm::Rs384,
        JwsAlgorithm::Rs512,
        JwsAlgorithm::Ps256,
        JwsAlgorithm::Ps384,
        JwsAlgorithm::Ps512,
        JwsAlgorithm::Es256,
        JwsAlgorithm::Es384,
        JwsAlgorithm::Es512,
    ];

    /// The `alg` header value, e.g. `"HS256"`.
    pub fn label(self) -> &'static str {
        match self {
            JwsAlgorithm::Hs256 => "HS256",
            JwsAlgorithm::Hs384 => "HS384",
            JwsAlgorithm::Hs512 => "HS512",
            JwsAlgorithm::Rs256 => "RS256",
            JwsAlgorithm::Rs384 => "RS384",
            JwsAlgorithm::Rs512 => "RS512",
            JwsAlgorithm::Ps256 => "PS256",
            JwsAlgorithm::Ps384 => "PS384",
            JwsAlgorithm::Ps512 => "PS512",
            JwsAlgorithm::Es256 => "ES256",
            JwsAlgorithm::Es384 => "ES384",
            JwsAlgorithm::Es512 => "ES512",
        }
    }

    /// A short human description, e.g. `"HMAC-SHA256"` or `"P-256"`.
    pub fn description(self) -> &'static str {
        match self {
            JwsAlgorithm::Hs256 => "HMAC-SHA256",
            JwsAlgorithm::Hs384 => "HMAC-SHA384",
            JwsAlgorithm::Hs512 => "HMAC-SHA512",
            JwsAlgorithm::Rs256 => "RSA-SHA256",
            JwsAlgorithm::Rs384 => "RSA-SHA384",
            JwsAlgorithm::Rs512 => "RSA-SHA512",
            JwsAlgorithm::Ps256 => "RSA-PSS-SHA256",
            JwsAlgorithm::Ps384 => "RSA-PSS-SHA384",
            JwsAlgorithm::Ps512 => "RSA-PSS-SHA512",
            JwsAlgorithm::Es256 => "P-256",
            JwsAlgorithm::Es384 => "P-384",
            JwsAlgorithm::Es512 => "P-521",
        }
    }

    /// Matches the small badges shown on the reference site (8gwifi.org's JWS generator),
    /// e.g. "Recommended" on PS256. Empty string for algorithms with no badge.
    pub fn badge(self) -> &'static str {
        match self {
            JwsAlgorithm::Ps256 => "Recommended",
            JwsAlgorithm::Es256 => "Best Performance",
            JwsAlgorithm::Es512 => "Highest Security",
            _ => "",
        }
    }

    pub fn family(self) -> JwsAlgorithmFamily {
        match self {
            JwsAlgorithm::Hs256 | JwsAlgorithm::Hs384 | JwsAlgorithm::Hs512 => {
                JwsAlgorithmFamily::Hmac
            }
            JwsAlgorithm::Rs256 | JwsAlgorithm::Rs384 | JwsAlgorithm::Rs512 => {
                JwsAlgorithmFamily::RsaPkcs1
            }
            JwsAlgorithm::Ps256 | JwsAlgorithm::Ps384 | JwsAlgorithm::Ps512 => {
                JwsAlgorithmFamily::RsaPss
            }
            JwsAlgorithm::Es256 | JwsAlgorithm::Es384 | JwsAlgorithm::Es512 => {
                JwsAlgorithmFamily::Ecdsa
            }
        }
    }

    /// Looks up an algorithm by its `alg` header value (case-insensitive), e.g. for
    /// dispatching based on a JWS's own header when verifying.
    pub fn from_label(label: &str) -> Option<Self> {
        JwsAlgorithm::ALL
            .into_iter()
            .find(|algo| algo.label().eq_ignore_ascii_case(label))
    }
}

/// Signs `payload` (treated as a raw octet string, per RFC 7515 - it doesn't have to be
/// JSON) with `key_input` using `algorithm`, returning the compact
/// `header.payload.signature` JWS.
pub fn generate_jws(
    algorithm: JwsAlgorithm,
    key_input: &str,
    payload: &str,
) -> Result<String, String> {
    build_jws(algorithm, key_input, payload, false)
}

/// Like `generate_jws`, but produces a JWS with a *detached* payload: the payload is
/// still signed, but omitted from the output, giving a two-segment `header.signature`
/// (rather than the usual three-segment `header.payload.signature`). Useful when the
/// payload is large or already transmitted/stored separately (e.g. signing an HTTP
/// message body) - a verifier must be given the original payload out-of-band to check the
/// signature.
pub fn generate_jws_detached(
    algorithm: JwsAlgorithm,
    key_input: &str,
    payload: &str,
) -> Result<String, String> {
    build_jws(algorithm, key_input, payload, true)
}

fn build_jws(
    algorithm: JwsAlgorithm,
    key_input: &str,
    payload: &str,
    detached: bool,
) -> Result<String, String> {
    let key_input = key_input.trim();
    if key_input.is_empty() {
        return Err("Key is required".to_string());
    }

    let header = format!("{{\"alg\":\"{}\",\"typ\":\"JWT\"}}", algorithm.label());
    let header_b64 = base64url_encode(header.as_bytes());
    let payload_b64 = base64url_encode(payload.as_bytes());
    let signing_input = format!("{header_b64}.{payload_b64}");

    let signature = sign(algorithm, key_input, signing_input.as_bytes())?;
    let signature_b64 = base64url_encode(&signature);
    if detached {
        Ok(format!("{header_b64}.{signature_b64}"))
    } else {
        Ok(format!("{header_b64}.{payload_b64}.{signature_b64}"))
    }
}

/// A lightweight look at a JWS's header, computed without needing a key - used by the
/// verify tool to show the detected algorithm and whether the payload is detached before
/// the user supplies a key (and, if detached, the original payload).
pub struct JwsPreview {
    pub algorithm: Option<JwsAlgorithm>,
    pub alg_label: String,
    pub is_detached: bool,
}

/// Splits `token` into its segments and decodes just enough of the header to report the
/// `alg` and whether the payload is detached (no payload segment at all).
pub fn inspect_jws(token: &str) -> Result<JwsPreview, String> {
    let (header_b64, payload_b64, _signature_b64) = split_jws(token)?;
    let alg_label = read_alg(header_b64)?;
    Ok(JwsPreview {
        algorithm: JwsAlgorithm::from_label(&alg_label),
        alg_label,
        is_detached: payload_b64.is_none(),
    })
}

/// Verifies a JWS against `key_input`, using the algorithm named in the token's own
/// header. Accepts both the normal three-segment `header.payload.signature` form and a
/// detached two-segment `header.signature` form; `detached_payload` must be supplied (as
/// the original raw payload text) for the latter and is ignored for the former.
pub fn verify_jws(
    token: &str,
    key_input: &str,
    detached_payload: Option<&str>,
) -> Result<bool, String> {
    let key_input = key_input.trim();
    if key_input.is_empty() {
        return Err("Key is required".to_string());
    }

    let (header_b64, payload_b64, signature_b64) = split_jws(token)?;
    let alg_label = read_alg(header_b64)?;
    let algorithm = JwsAlgorithm::from_label(&alg_label)
        .ok_or_else(|| format!("Unsupported or unknown algorithm \"{alg_label}\""))?;

    let payload_b64 = match payload_b64 {
        Some(p) => p.to_string(),
        None => {
            let payload = detached_payload.filter(|p| !p.is_empty()).ok_or(
                "This JWS has a detached payload - paste the original payload to verify it",
            )?;
            base64url_encode(payload.as_bytes())
        }
    };

    let signing_input = format!("{header_b64}.{payload_b64}");
    let signature =
        base64url_decode(signature_b64).map_err(|e| format!("Invalid signature segment: {e}"))?;
    verify(algorithm, key_input, signing_input.as_bytes(), &signature)
}

/// Splits a compact JWS into its (still base64url-encoded) header/payload/signature
/// segments. Accepts the normal three-segment `header.payload.signature` form (`payload`
/// is `None` if that segment happens to be empty, RFC 7515 Appendix F style) as well as a
/// detached two-segment `header.signature` form (`payload` is always `None`).
fn split_jws(token: &str) -> Result<(&str, Option<&str>, &str), String> {
    let token = token.trim();
    let parts: Vec<&str> = token.split('.').collect();
    match parts.as_slice() {
        [header, payload, signature] if payload.is_empty() => Ok((header, None, signature)),
        [header, payload, signature] => Ok((header, Some(*payload), signature)),
        [header, signature] => Ok((header, None, signature)),
        _ => Err(format!(
            "Not a valid JWS: expected \"header.payload.signature\" (or \"header.signature\" for a detached payload), found {} part(s)",
            parts.len()
        )),
    }
}

fn read_alg(header_b64: &str) -> Result<String, String> {
    let header_bytes =
        base64url_decode(header_b64).map_err(|e| format!("Invalid header segment: {e}"))?;
    let header_value: serde_json::Value = serde_json::from_slice(&header_bytes)
        .map_err(|e| format!("Header isn't valid JSON: {e}"))?;
    header_value
        .get("alg")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| "Header is missing an \"alg\" field".to_string())
}

fn verify(
    algorithm: JwsAlgorithm,
    key_input: &str,
    data: &[u8],
    signature: &[u8],
) -> Result<bool, String> {
    match algorithm {
        JwsAlgorithm::Hs256 => Ok(hmac_sha256_sign(key_input.as_bytes(), data) == signature),
        JwsAlgorithm::Hs384 => Ok(hmac_sha384_sign(key_input.as_bytes(), data) == signature),
        JwsAlgorithm::Hs512 => Ok(hmac_sha512_sign(key_input.as_bytes(), data) == signature),
        JwsAlgorithm::Rs256 => rsa_pkcs1_verify_sha256(key_input, data, signature),
        JwsAlgorithm::Rs384 => rsa_pkcs1_verify_sha384(key_input, data, signature),
        JwsAlgorithm::Rs512 => rsa_pkcs1_verify_sha512(key_input, data, signature),
        JwsAlgorithm::Ps256 => rsa_pss_verify_sha256(key_input, data, signature),
        JwsAlgorithm::Ps384 => rsa_pss_verify_sha384(key_input, data, signature),
        JwsAlgorithm::Ps512 => rsa_pss_verify_sha512(key_input, data, signature),
        JwsAlgorithm::Es256 => ecdsa_p256_verify(key_input, data, signature),
        JwsAlgorithm::Es384 => ecdsa_p384_verify(key_input, data, signature),
        JwsAlgorithm::Es512 => ecdsa_p521_verify(key_input, data, signature),
    }
}

/// Parses a PEM-encoded RSA public key, accepting SPKI (`BEGIN PUBLIC KEY`) or PKCS#1
/// (`BEGIN RSA PUBLIC KEY`); as a convenience, a private key (PKCS#8 or PKCS#1) is also
/// accepted and its public half is used, since that's often what's on hand for testing.
fn parse_rsa_public_key(pem: &str) -> Result<rsa::RsaPublicKey, String> {
    use rsa::pkcs1::DecodeRsaPublicKey;
    use rsa::pkcs8::DecodePublicKey;

    let pem = pem.trim();
    if let Ok(key) = rsa::RsaPublicKey::from_public_key_pem(pem) {
        return Ok(key);
    }
    if let Ok(key) = rsa::RsaPublicKey::from_pkcs1_pem(pem) {
        return Ok(key);
    }
    if let Ok(key) = parse_rsa_private_key(pem) {
        return Ok(key.to_public_key());
    }
    Err("Invalid RSA public key - expected PEM, SPKI (\"BEGIN PUBLIC KEY\"), PKCS#1 (\"BEGIN RSA PUBLIC KEY\"), or a private key".to_string())
}

fn rsa_pkcs1_verify_sha256(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::signature::Verifier;
    let verifying_key = VerifyingKey::<Sha256>::new(parse_rsa_public_key(key_pem)?);
    let sig = Signature::try_from(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn rsa_pkcs1_verify_sha384(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::signature::Verifier;
    let verifying_key = VerifyingKey::<Sha384>::new(parse_rsa_public_key(key_pem)?);
    let sig = Signature::try_from(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn rsa_pkcs1_verify_sha512(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::signature::Verifier;
    let verifying_key = VerifyingKey::<Sha512>::new(parse_rsa_public_key(key_pem)?);
    let sig = Signature::try_from(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn rsa_pss_verify_sha256(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use rsa::pss::{Signature, VerifyingKey};
    use rsa::signature::Verifier;
    let verifying_key = VerifyingKey::<Sha256>::new(parse_rsa_public_key(key_pem)?);
    let sig = Signature::try_from(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn rsa_pss_verify_sha384(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use rsa::pss::{Signature, VerifyingKey};
    use rsa::signature::Verifier;
    let verifying_key = VerifyingKey::<Sha384>::new(parse_rsa_public_key(key_pem)?);
    let sig = Signature::try_from(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn rsa_pss_verify_sha512(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use rsa::pss::{Signature, VerifyingKey};
    use rsa::signature::Verifier;
    let verifying_key = VerifyingKey::<Sha512>::new(parse_rsa_public_key(key_pem)?);
    let sig = Signature::try_from(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

/// Parses a P-256 public key, accepting SPKI PEM directly or (as a convenience) a private
/// key PEM, from which the public key is derived.
fn parse_p256_verifying_key(pem: &str) -> Result<p256::ecdsa::VerifyingKey, String> {
    use p256::ecdsa::VerifyingKey;
    use p256::elliptic_curve::pkcs8::DecodePublicKey;

    let pem = pem.trim();
    if let Ok(key) = VerifyingKey::from_public_key_pem(pem) {
        return Ok(key);
    }
    if let Ok(secret_key) = p256::SecretKey::from_pem(pem) {
        return Ok(VerifyingKey::from(secret_key.public_key()));
    }
    Err(
        "Invalid P-256 public key - expected PEM, SPKI (\"BEGIN PUBLIC KEY\"), or a private key"
            .to_string(),
    )
}

fn parse_p384_verifying_key(pem: &str) -> Result<p384::ecdsa::VerifyingKey, String> {
    use p384::ecdsa::VerifyingKey;
    use p384::elliptic_curve::pkcs8::DecodePublicKey;

    let pem = pem.trim();
    if let Ok(key) = VerifyingKey::from_public_key_pem(pem) {
        return Ok(key);
    }
    if let Ok(secret_key) = p384::SecretKey::from_pem(pem) {
        return Ok(VerifyingKey::from(secret_key.public_key()));
    }
    Err(
        "Invalid P-384 public key - expected PEM, SPKI (\"BEGIN PUBLIC KEY\"), or a private key"
            .to_string(),
    )
}

fn parse_p521_verifying_key(pem: &str) -> Result<p521::ecdsa::VerifyingKey, String> {
    use p521::ecdsa::VerifyingKey;
    use p521::elliptic_curve::pkcs8::DecodePublicKey;

    let pem = pem.trim();
    if let Ok(key) = VerifyingKey::from_public_key_pem(pem) {
        return Ok(key);
    }
    if let Ok(secret_key) = p521::SecretKey::from_pem(pem) {
        return Ok(VerifyingKey::from(secret_key.public_key()));
    }
    Err(
        "Invalid P-521 public key - expected PEM, SPKI (\"BEGIN PUBLIC KEY\"), or a private key"
            .to_string(),
    )
}

fn ecdsa_p256_verify(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use p256::ecdsa::{Signature, signature::Verifier};
    let verifying_key = parse_p256_verifying_key(key_pem)?;
    let sig = Signature::from_slice(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn ecdsa_p384_verify(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use p384::ecdsa::{Signature, signature::Verifier};
    let verifying_key = parse_p384_verifying_key(key_pem)?;
    let sig = Signature::from_slice(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn ecdsa_p521_verify(key_pem: &str, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    use p521::ecdsa::{Signature, signature::Verifier};
    let verifying_key = parse_p521_verifying_key(key_pem)?;
    let sig = Signature::from_slice(signature).map_err(|e| format!("Invalid signature: {e}"))?;
    Ok(verifying_key.verify(data, &sig).is_ok())
}

fn sign(algorithm: JwsAlgorithm, key_input: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    match algorithm {
        JwsAlgorithm::Hs256 => Ok(hmac_sha256_sign(key_input.as_bytes(), data)),
        JwsAlgorithm::Hs384 => Ok(hmac_sha384_sign(key_input.as_bytes(), data)),
        JwsAlgorithm::Hs512 => Ok(hmac_sha512_sign(key_input.as_bytes(), data)),
        JwsAlgorithm::Rs256 => rsa_pkcs1_sign_sha256(key_input, data),
        JwsAlgorithm::Rs384 => rsa_pkcs1_sign_sha384(key_input, data),
        JwsAlgorithm::Rs512 => rsa_pkcs1_sign_sha512(key_input, data),
        JwsAlgorithm::Ps256 => rsa_pss_sign_sha256(key_input, data),
        JwsAlgorithm::Ps384 => rsa_pss_sign_sha384(key_input, data),
        JwsAlgorithm::Ps512 => rsa_pss_sign_sha512(key_input, data),
        JwsAlgorithm::Es256 => ecdsa_p256_sign(key_input, data),
        JwsAlgorithm::Es384 => ecdsa_p384_sign(key_input, data),
        JwsAlgorithm::Es512 => ecdsa_p521_sign(key_input, data),
    }
}

// HMAC accepts keys of any length (longer keys get hashed down internally), so these
// `new_from_slice` calls cannot fail in practice. Written as three concrete functions
// (rather than one generic over the digest) to sidestep hmac/digest's fiddly trait
// bounds - same approach `hashing.rs` uses.
fn hmac_sha256_sign(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha384_sign(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha384>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha512_sign(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha512>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Parses a PEM-encoded RSA private key, trying PKCS#8 (`BEGIN PRIVATE KEY`) first and
/// falling back to PKCS#1 (`BEGIN RSA PRIVATE KEY`), since both are common.
fn parse_rsa_private_key(pem: &str) -> Result<RsaPrivateKey, String> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    let pem = pem.trim();
    if let Ok(key) = RsaPrivateKey::from_pkcs8_pem(pem) {
        return Ok(key);
    }
    RsaPrivateKey::from_pkcs1_pem(pem).map_err(|_| {
        "Invalid RSA private key - expected PEM, either PKCS#1 (\"BEGIN RSA PRIVATE KEY\") or PKCS#8 (\"BEGIN PRIVATE KEY\")".to_string()
    })
}

fn rsa_pkcs1_sign_sha256(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = rsa::pkcs1v15::SigningKey::<Sha256>::new(parse_rsa_private_key(key_pem)?);
    signing_key
        .try_sign(data)
        .map(|sig| sig.to_vec())
        .map_err(|e| format!("RSA signing failed: {e}"))
}

fn rsa_pkcs1_sign_sha384(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = rsa::pkcs1v15::SigningKey::<Sha384>::new(parse_rsa_private_key(key_pem)?);
    signing_key
        .try_sign(data)
        .map(|sig| sig.to_vec())
        .map_err(|e| format!("RSA signing failed: {e}"))
}

fn rsa_pkcs1_sign_sha512(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = rsa::pkcs1v15::SigningKey::<Sha512>::new(parse_rsa_private_key(key_pem)?);
    signing_key
        .try_sign(data)
        .map(|sig| sig.to_vec())
        .map_err(|e| format!("RSA signing failed: {e}"))
}

fn rsa_pss_sign_sha256(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = rsa::pss::SigningKey::<Sha256>::new(parse_rsa_private_key(key_pem)?);
    let signature: rsa::pss::Signature = signing_key
        .try_sign(data)
        .map_err(|e| format!("RSA-PSS signing failed: {e}"))?;
    Ok(signature.to_vec())
}

fn rsa_pss_sign_sha384(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = rsa::pss::SigningKey::<Sha384>::new(parse_rsa_private_key(key_pem)?);
    let signature: rsa::pss::Signature = signing_key
        .try_sign(data)
        .map_err(|e| format!("RSA-PSS signing failed: {e}"))?;
    Ok(signature.to_vec())
}

fn rsa_pss_sign_sha512(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = rsa::pss::SigningKey::<Sha512>::new(parse_rsa_private_key(key_pem)?);
    let signature: rsa::pss::Signature = signing_key
        .try_sign(data)
        .map_err(|e| format!("RSA-PSS signing failed: {e}"))?;
    Ok(signature.to_vec())
}

fn ecdsa_p256_sign(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use p256::ecdsa::{Signature, SigningKey, signature::Signer};
    let secret_key = p256::SecretKey::from_pem(key_pem.trim())
        .map_err(|e| format!("Invalid P-256 EC private key: {e}"))?;
    let signing_key = SigningKey::from(secret_key);
    let signature: Signature = signing_key
        .try_sign(data)
        .map_err(|e| format!("ECDSA signing failed: {e}"))?;
    Ok(signature.to_vec())
}

fn ecdsa_p384_sign(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use p384::ecdsa::{Signature, SigningKey, signature::Signer};
    let secret_key = p384::SecretKey::from_pem(key_pem.trim())
        .map_err(|e| format!("Invalid P-384 EC private key: {e}"))?;
    let signing_key = SigningKey::from(secret_key);
    let signature: Signature = signing_key
        .try_sign(data)
        .map_err(|e| format!("ECDSA signing failed: {e}"))?;
    Ok(signature.to_vec())
}

fn ecdsa_p521_sign(key_pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use p521::ecdsa::{Signature, SigningKey, signature::Signer};
    let secret_key = p521::SecretKey::from_pem(key_pem.trim())
        .map_err(|e| format!("Invalid P-521 EC private key: {e}"))?;
    let signing_key = SigningKey::from(secret_key);
    let signature: Signature = signing_key
        .try_sign(data)
        .map_err(|e| format!("ECDSA signing failed: {e}"))?;
    Ok(signature.to_vec())
}

fn base64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes a base64url segment, tolerating both the padding-free form JWTs normally use
/// and a padded fallback for slightly non-compliant tokens (mirrors `jwt.rs`'s decoder).
fn base64url_decode(segment: &str) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(segment) {
        return Ok(bytes);
    }
    base64::engine::general_purpose::URL_SAFE
        .decode(segment)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs256_matches_known_vector() {
        // Same classic jwt.io example used in jwt.rs's tests.
        let jws = generate_jws(
            JwsAlgorithm::Hs256,
            "your-256-bit-secret",
            r#"{"sub":"1234567890","name":"John Doe","iat":1516239022}"#,
        )
        .unwrap();
        assert_eq!(
            jws,
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        assert!(crate::jwt::verify_jwt_hmac(&jws, "your-256-bit-secret") == Ok(true));
    }

    #[test]
    fn empty_key_is_rejected() {
        assert!(generate_jws(JwsAlgorithm::Hs256, "", "payload").is_err());
    }

    #[test]
    fn rsa_pkcs1_round_trips_through_rsa_verify() {
        use rsa::pkcs1v15::VerifyingKey;
        use rsa::signature::Verifier;

        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        use rsa::pkcs8::EncodePrivateKey;
        let pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Rs256, &pem, "hello").unwrap();
        let mut parts = jws.split('.');
        let header = parts.next().unwrap();
        let payload = parts.next().unwrap();
        let sig_b64 = parts.next().unwrap();
        let signing_input = format!("{header}.{payload}");
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .unwrap();
        let signature = rsa::pkcs1v15::Signature::try_from(sig_bytes.as_slice()).unwrap();

        let verifying_key = VerifyingKey::<Sha256>::new(priv_key.to_public_key());
        assert!(
            verifying_key
                .verify(signing_input.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn rsa_pss_round_trips_through_rsa_verify() {
        use rsa::pkcs8::EncodePrivateKey;
        use rsa::pss::VerifyingKey;
        use rsa::signature::Verifier;

        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Ps256, &pem, "hello").unwrap();
        let mut parts = jws.split('.');
        let header = parts.next().unwrap();
        let payload = parts.next().unwrap();
        let sig_b64 = parts.next().unwrap();
        let signing_input = format!("{header}.{payload}");
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .unwrap();
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();

        let verifying_key = VerifyingKey::<Sha256>::new(priv_key.to_public_key());
        assert!(
            verifying_key
                .verify(signing_input.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn ecdsa_es256_round_trips_through_verify() {
        use p256::ecdsa::{Signature, SigningKey, VerifyingKey, signature::Verifier};
        use p256::elliptic_curve::{Generate, pkcs8::EncodePrivateKey};

        let signing_key = SigningKey::generate();
        let pem = signing_key
            .to_pkcs8_pem(Default::default())
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Es256, &pem, "hello").unwrap();
        let mut parts = jws.split('.');
        let header = parts.next().unwrap();
        let payload = parts.next().unwrap();
        let sig_b64 = parts.next().unwrap();
        let signing_input = format!("{header}.{payload}");
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .unwrap();
        let signature = Signature::from_slice(&sig_bytes).unwrap();

        let verifying_key = VerifyingKey::from(&signing_key);
        assert!(
            verifying_key
                .verify(signing_input.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn ecdsa_es384_round_trips_through_verify() {
        use p384::ecdsa::{Signature, SigningKey, VerifyingKey, signature::Verifier};
        use p384::elliptic_curve::{Generate, pkcs8::EncodePrivateKey};

        let signing_key = SigningKey::generate();
        let pem = signing_key
            .to_pkcs8_pem(Default::default())
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Es384, &pem, "hello").unwrap();
        let mut parts = jws.split('.');
        let header = parts.next().unwrap();
        let payload = parts.next().unwrap();
        let sig_b64 = parts.next().unwrap();
        let signing_input = format!("{header}.{payload}");
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .unwrap();
        let signature = Signature::from_slice(&sig_bytes).unwrap();

        let verifying_key = VerifyingKey::from(&signing_key);
        assert!(
            verifying_key
                .verify(signing_input.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn ecdsa_es512_round_trips_through_verify() {
        use p521::ecdsa::{Signature, SigningKey, VerifyingKey, signature::Verifier};
        use p521::elliptic_curve::{Generate, pkcs8::EncodePrivateKey};

        let signing_key = SigningKey::generate();
        let pem = signing_key
            .to_pkcs8_pem(Default::default())
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Es512, &pem, "hello").unwrap();
        let mut parts = jws.split('.');
        let header = parts.next().unwrap();
        let payload = parts.next().unwrap();
        let sig_b64 = parts.next().unwrap();
        let signing_input = format!("{header}.{payload}");
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .unwrap();
        let signature = Signature::from_slice(&sig_bytes).unwrap();

        let verifying_key = VerifyingKey::from(&signing_key);
        assert!(
            verifying_key
                .verify(signing_input.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn verify_jws_accepts_valid_hmac_and_rejects_wrong_key() {
        let jws = generate_jws(JwsAlgorithm::Hs256, "correct-secret", "hello").unwrap();
        assert_eq!(verify_jws(&jws, "correct-secret", None), Ok(true));
        assert_eq!(verify_jws(&jws, "wrong-secret", None), Ok(false));
    }

    #[test]
    fn verify_jws_round_trips_rsa_pkcs1_with_public_key() {
        use rsa::pkcs8::EncodePrivateKey;
        use rsa::pkcs8::EncodePublicKey;

        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let priv_pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();
        let pub_pem = priv_key
            .to_public_key()
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap();

        let jws = generate_jws(JwsAlgorithm::Rs256, &priv_pem, "hello").unwrap();
        assert_eq!(verify_jws(&jws, &pub_pem, None), Ok(true));
        // The private key also works as a verify-side convenience.
        assert_eq!(verify_jws(&jws, &priv_pem, None), Ok(true));
    }

    #[test]
    fn verify_jws_round_trips_rsa_pss() {
        use rsa::pkcs8::EncodePrivateKey;

        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Ps256, &pem, "hello").unwrap();
        assert_eq!(verify_jws(&jws, &pem, None), Ok(true));
    }

    #[test]
    fn verify_jws_round_trips_ecdsa_es256() {
        use p256::ecdsa::SigningKey;
        use p256::elliptic_curve::{Generate, pkcs8::EncodePrivateKey};

        let signing_key = SigningKey::generate();
        let pem = signing_key
            .to_pkcs8_pem(Default::default())
            .unwrap()
            .to_string();

        let jws = generate_jws(JwsAlgorithm::Es256, &pem, "hello").unwrap();
        assert_eq!(verify_jws(&jws, &pem, None), Ok(true));
    }

    #[test]
    fn verify_jws_rejects_tampered_payload() {
        let jws = generate_jws(JwsAlgorithm::Hs256, "secret", "hello").unwrap();
        let mut parts: Vec<&str> = jws.split('.').collect();
        let tampered_payload = base64url_encode(b"goodbye");
        parts[1] = &tampered_payload;
        let tampered = parts.join(".");
        assert_eq!(verify_jws(&tampered, "secret", None), Ok(false));
    }

    #[test]
    fn detached_jws_round_trips_through_verify() {
        let jws = generate_jws_detached(JwsAlgorithm::Hs256, "secret", "the payload").unwrap();
        let parts: Vec<&str> = jws.split('.').collect();
        assert_eq!(
            parts.len(),
            2,
            "detached JWS should be just header.signature"
        );
        assert!(parts[0].starts_with("eyJ"));

        let preview = inspect_jws(&jws).unwrap();
        assert!(preview.is_detached);
        assert_eq!(preview.algorithm, Some(JwsAlgorithm::Hs256));

        // Verifying without the original payload fails with a helpful error...
        assert!(verify_jws(&jws, "secret", None).is_err());
        // ...but succeeds once it's supplied out-of-band.
        assert_eq!(verify_jws(&jws, "secret", Some("the payload")), Ok(true));
        // A wrong payload should not verify.
        assert_eq!(
            verify_jws(&jws, "secret", Some("not the payload")),
            Ok(false)
        );
    }
}
