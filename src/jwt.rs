use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Sha256, Sha384, Sha512};

/// Result of successfully splitting and decoding a JWT's three segments.
pub struct JwtDecoded {
    pub algorithm: Option<String>,
    pub header_json: String,
    pub payload_json: String,
    pub signature_b64: String,
    /// Registered JWT claims found in the payload (RFC 7519 §4.1), as `(label, value)`
    /// pairs in the conventional `iss`/`sub`/`aud`/`exp`/`nbf`/`iat`/`jti` order - mirrors
    /// the claims table on 8gwifi.org's JWS parser. Empty when the payload isn't a JSON
    /// object or has none of these claims.
    pub claims: Vec<(&'static str, String)>,
    /// Human-readable notes: expiry status (if `exp` is present) and a note when the
    /// payload isn't JSON.
    pub claim_notes: Vec<String>,
}

/// Decodes (but does not cryptographically verify) a `header.payload.signature` JWT,
/// pretty-printing the header/payload JSON the way jwt.io's debugger does.
pub fn decode_jwt(token: &str) -> Result<JwtDecoded, String> {
    let token = token.trim();
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "Not a valid JWT: expected 3 dot-separated parts (header.payload.signature), found {}",
            parts.len()
        ));
    }

    let header_bytes =
        base64url_decode(parts[0]).map_err(|e| format!("Invalid header segment: {e}"))?;
    let payload_bytes =
        base64url_decode(parts[1]).map_err(|e| format!("Invalid payload segment: {e}"))?;

    let header_value: Value = serde_json::from_slice(&header_bytes)
        .map_err(|e| format!("Header isn't valid JSON: {e}"))?;

    let algorithm = header_value
        .get("alg")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let header_json = serde_json::to_string_pretty(&header_value)
        .map_err(|e| format!("Failed to format header: {e}"))?;

    // The payload doesn't have to be JSON - RFC 7515 allows any octet string as the
    // payload of a JWS, and some issuers (e.g. opaque/plain-string payloads) use that.
    // Pretty-print it when it happens to be JSON, otherwise fall back to showing the raw
    // decoded text so the token can still be inspected instead of failing outright.
    let payload_value: Option<Value> = serde_json::from_slice(&payload_bytes).ok();
    let payload_json = match &payload_value {
        Some(value) => serde_json::to_string_pretty(value)
            .map_err(|e| format!("Failed to format payload: {e}"))?,
        None => String::from_utf8_lossy(&payload_bytes).into_owned(),
    };

    let mut claims = Vec::new();
    let mut claim_notes = Vec::new();
    if let Some(payload_value) = &payload_value {
        if let Some(iss) = payload_value.get("iss").and_then(Value::as_str) {
            claims.push(("iss (Issuer)", iss.to_string()));
        }
        if let Some(sub) = payload_value.get("sub").and_then(Value::as_str) {
            claims.push(("sub (Subject)", sub.to_string()));
        }
        if let Some(aud) = payload_value.get("aud") {
            let aud_display = match aud {
                Value::String(s) => Some(s.clone()),
                Value::Array(items) => {
                    let joined = items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    (!joined.is_empty()).then_some(joined)
                }
                _ => None,
            };
            if let Some(aud_display) = aud_display {
                claims.push(("aud (Audience)", aud_display));
            }
        }
        for (key, label) in [
            ("exp", "exp (Expiration Time)"),
            ("nbf", "nbf (Not Before)"),
            ("iat", "iat (Issued At)"),
        ] {
            if let Some(seconds) = payload_value.get(key).and_then(Value::as_i64) {
                claims.push((
                    label,
                    format!("{seconds} ({})", format_unix_timestamp(seconds)),
                ));
            }
        }
        if let Some(jti) = payload_value.get("jti").and_then(Value::as_str) {
            claims.push(("jti (JWT ID)", jti.to_string()));
        }

        if let Some(exp) = payload_value.get("exp").and_then(Value::as_i64) {
            claim_notes.push(if exp < current_unix_time() {
                "⚠ Token is EXPIRED".to_string()
            } else {
                "✓ Token is not expired".to_string()
            });
        }
    } else {
        claim_notes.push("Payload isn't JSON - showing the raw decoded text".to_string());
    }

    Ok(JwtDecoded {
        algorithm,
        header_json,
        payload_json,
        signature_b64: parts[2].to_string(),
        claims,
        claim_notes,
    })
}

/// Recomputes the HMAC signature for `token` using `secret` and checks it against the
/// signature segment. Only HS256/HS384/HS512 are supported (asymmetric algorithms like
/// RS256/ES256 need a public key, not a shared secret, so they're reported as errors).
pub fn verify_jwt_hmac(token: &str, secret: &str) -> Result<bool, String> {
    let token = token.trim();
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("Not a valid JWT: expected 3 dot-separated parts".to_string());
    }

    let header_bytes =
        base64url_decode(parts[0]).map_err(|e| format!("Invalid header segment: {e}"))?;
    let header_value: Value = serde_json::from_slice(&header_bytes)
        .map_err(|e| format!("Header isn't valid JSON: {e}"))?;
    let alg = header_value
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_uppercase();

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let key_bytes = secret.as_bytes();
    let computed: Vec<u8> = match alg.as_str() {
        "HS256" => {
            let mut mac =
                Hmac::<Sha256>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(signing_input.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }
        "HS384" => {
            let mut mac =
                Hmac::<Sha384>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(signing_input.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }
        "HS512" => {
            let mut mac =
                Hmac::<Sha512>::new_from_slice(key_bytes).expect("HMAC accepts any key length");
            mac.update(signing_input.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }
        other => {
            return Err(format!(
                "Verification isn't supported for alg \"{other}\" (only HS256/HS384/HS512 shared-secret algorithms)"
            ));
        }
    };

    let actual =
        base64url_decode(parts[2]).map_err(|e| format!("Invalid signature segment: {e}"))?;
    Ok(computed == actual)
}

/// Decodes a base64url segment, tolerating both the padding-free form JWTs normally use
/// and a padded fallback for slightly non-compliant tokens.
fn base64url_decode(segment: &str) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(segment) {
        return Ok(bytes);
    }
    base64::engine::general_purpose::URL_SAFE
        .decode(segment)
        .map_err(|e| e.to_string())
}

fn current_unix_time() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Formats a Unix timestamp (seconds) as `YYYY-MM-DD HH:MM:SS UTC` without depending on a
/// date/time crate, using Howard Hinnant's civil-from-days algorithm
/// (http://howardhinnant.github.io/date_algorithms.html, public domain).
fn format_unix_timestamp(total_seconds: i64) -> String {
    let days = total_seconds.div_euclid(86400);
    let secs_of_day = total_seconds.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    let ss = secs_of_day % 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} UTC")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Classic jwt.io example token: alg HS256, payload {"sub":"1234567890","name":"John Doe","iat":1516239022}
    const SAMPLE: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

    #[test]
    fn decodes_header_and_payload() {
        let decoded = decode_jwt(SAMPLE).unwrap();
        assert_eq!(decoded.algorithm.as_deref(), Some("HS256"));
        assert!(decoded.payload_json.contains("\"name\": \"John Doe\""));
        assert!(decoded.payload_json.contains("\"sub\": \"1234567890\""));
        assert!(
            decoded
                .claims
                .iter()
                .any(|(label, value)| *label == "sub (Subject)" && value == "1234567890")
        );
        assert!(
            decoded
                .claims
                .iter()
                .any(|(label, _)| *label == "iat (Issued At)")
        );
    }

    #[test]
    fn rejects_malformed_token() {
        assert!(decode_jwt("not-a-jwt").is_err());
        assert!(decode_jwt("a.b").is_err());
    }

    #[test]
    fn decodes_claims_table_with_issuer_audience_array_and_jti() {
        let payload = r#"{"iss":"my-issuer","aud":["service-a","service-b"],"jti":"abc-123"}"#;
        let token =
            crate::jws::generate_jws(crate::jws::JwsAlgorithm::Hs256, "secret", payload).unwrap();
        let decoded = decode_jwt(&token).unwrap();
        assert!(
            decoded
                .claims
                .contains(&("iss (Issuer)", "my-issuer".to_string()))
        );
        assert!(
            decoded
                .claims
                .contains(&("aud (Audience)", "service-a, service-b".to_string()))
        );
        assert!(
            decoded
                .claims
                .contains(&("jti (JWT ID)", "abc-123".to_string()))
        );
    }

    #[test]
    fn decodes_non_json_payload_as_raw_text() {
        // Payload segment "YWRi" decodes to the plain string "adb", not JSON - the JWS
        // spec (RFC 7515) allows an arbitrary octet string here, so this must still
        // decode instead of erroring out.
        let token = "eyJraWQiOiJkNGE0YjFjNS0yOTNlLTQxNzItODJlZi1jNjQ5OTJmNzU5MTciLCJhbGciOiJFUzUxMiJ9.YWRi.AUuhlafDtBMPNlwqigctU6wgWaUvB_UnnrcM3JEETioGTqAgp7BjHaYCqeDEGMiezJ7gROVIKxa9XkkfNkLgsmE6AI8w9pQXuv_XolFyjvSg2cDYT8KCXa-ESoLiF-2Y319MyO6MDc7LwSEN3-Hry2qSVGFuV-CpYLNeQYdEKpXufnUi";
        let decoded = decode_jwt(token).unwrap();
        assert_eq!(decoded.algorithm.as_deref(), Some("ES512"));
        assert_eq!(decoded.payload_json, "adb");
        assert!(decoded.claim_notes.iter().any(|n| n.contains("isn't JSON")));
    }

    #[test]
    fn verifies_correct_and_incorrect_secret() {
        assert_eq!(verify_jwt_hmac(SAMPLE, "your-256-bit-secret"), Ok(true));
        assert_eq!(verify_jwt_hmac(SAMPLE, "wrong-secret"), Ok(false));
    }

    #[test]
    fn unix_timestamp_formats_known_instant() {
        // 1516239022 is 2018-01-18 01:30:22 UTC.
        assert_eq!(format_unix_timestamp(1516239022), "2018-01-18 01:30:22 UTC");
    }
}
