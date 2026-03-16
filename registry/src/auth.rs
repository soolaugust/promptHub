use crate::db::Db;
use crate::error::{RegistryError, Result};
use rand::Rng;

/// Generate a random token with the "phrt_" prefix.
pub fn generate_token() -> String {
    let bytes: Vec<u8> = rand::thread_rng().sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .collect();
    format!("phrt_{}", String::from_utf8(bytes).unwrap())
}

/// Hash a password using bcrypt.
#[allow(dead_code)]
pub fn hash_password(password: &str) -> Result<String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST)
        .map_err(|e| RegistryError::Internal(e.to_string()))
}

/// Verify a password against a bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

/// Validate a Bearer token from the Authorization header.
/// Returns the user_id if valid, or Unauthorized error.
pub fn require_auth(db: &Db, auth_header: Option<&str>, admin_token: &str) -> Result<Option<i64>> {
    let token = extract_bearer(auth_header)
        .ok_or(RegistryError::Unauthorized)?;

    // Admin token (from config) is always valid
    if token == admin_token {
        return Ok(None); // None = admin, no user_id
    }

    // DB lookup
    match db.validate_token(token)? {
        Some((user_id, _)) => Ok(user_id),
        None => Err(RegistryError::Unauthorized),
    }
}

/// Extract the Bearer token string from an Authorization header value.
pub fn extract_bearer(header: Option<&str>) -> Option<&str> {
    let h = header?;
    h.strip_prefix("Bearer ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token_has_prefix() {
        let tok = generate_token();
        assert!(tok.starts_with("phrt_"), "token must start with phrt_");
        assert_eq!(tok.len(), 5 + 32);
    }

    #[test]
    fn test_hash_and_verify_password() {
        let hash = hash_password("mysecret").unwrap();
        assert!(verify_password("mysecret", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn test_extract_bearer_valid() {
        assert_eq!(extract_bearer(Some("Bearer phrt_abc")), Some("phrt_abc"));
    }

    #[test]
    fn test_extract_bearer_missing() {
        assert_eq!(extract_bearer(None), None);
        assert_eq!(extract_bearer(Some("Basic xyz")), None);
    }
}
