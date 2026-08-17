use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// JWT payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // username
    pub role: String, // "admin" | "user"
    pub exp: usize,   // expiry (unix seconds)
}

/// Hash a passcode with bcrypt (self-salting).
pub fn hash_passcode(plain: &str) -> Result<String> {
    bcrypt::hash(plain, bcrypt::DEFAULT_COST).map_err(|e| Error::Auth(e.to_string()))
}

/// Verify a passcode against a stored bcrypt hash.
pub fn verify_passcode(plain: &str, hash: &str) -> bool {
    bcrypt::verify(plain, hash).unwrap_or(false)
}

/// Mint an HS256 JWT valid for `days`.
pub fn make_token(username: &str, role: &str, secret: &str, days: i64) -> Result<String> {
    let exp = (chrono::Utc::now().timestamp() + days * 86_400).max(0) as usize;
    let claims = Claims {
        sub: username.to_string(),
        role: role.to_string(),
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| Error::Auth(e.to_string()))
}

/// Verify a token; `None` if invalid, tampered, or expired.
pub fn verify_token(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|d| d.claims)
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passcode_roundtrip() {
        let h = hash_passcode("hunter2").unwrap();
        assert!(verify_passcode("hunter2", &h));
        assert!(!verify_passcode("wrong", &h));
    }

    #[test]
    fn token_roundtrip_and_secret_check() {
        let t = make_token("saiz", "admin", "secret", 7).unwrap();
        let c = verify_token(&t, "secret").unwrap();
        assert_eq!(c.sub, "saiz");
        assert_eq!(c.role, "admin");
        assert!(verify_token(&t, "different-secret").is_none());
    }
}
