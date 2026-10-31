use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::http::{header, HeaderMap};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

const COOKIE: &str = "__Host-flwme_session";

pub fn normalize_email(raw: &str) -> Option<String> {
    let email = raw.trim().to_ascii_lowercase();
    if email.len() > 254 || email.len() < 5 || !email.is_ascii() || email.bytes().any(|b| b.is_ascii_whitespace() || b < 33) {
        return None;
    }
    let (local, domain) = email.split_once('@')?;
    if local.is_empty() || local.len() > 64 || domain.len() < 3 || domain.contains('@') || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return None;
    }
    Some(email)
}

pub fn valid_password(value: &str) -> bool {
    (10..=128).contains(&value.len()) && !value.chars().any(|c| c.is_control())
}

pub fn password_hash(password: &str) -> Result<String, ()> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|_| ())
}

pub fn password_matches(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

pub fn make_session() -> (String, Vec<u8>) {
    let mut raw = [0u8; 32];
    OsRng.fill_bytes(&mut raw);
    let token = hex::encode(raw);
    let digest = Sha256::digest(raw);
    (token, digest.to_vec())
}

pub fn hash_token(token: &str) -> Option<Vec<u8>> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let raw = hex::decode(token).ok()?;
    Some(Sha256::digest(&raw).to_vec())
}

pub fn cookie_from_headers(headers: &HeaderMap) -> Option<Vec<u8>> {
    let line = headers.get(header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{COOKIE}=");
    let token = line.split(';').map(str::trim).find_map(|part| part.strip_prefix(&prefix))?;
    hash_token(token)
}

pub fn session_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; Path=/; Max-Age=43200; HttpOnly; Secure; SameSite=Lax")
}

pub fn expire_cookie() -> &'static str {
    "__Host-flwme_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax"
}

pub fn valid_origin(headers: &HeaderMap) -> bool {
    headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) == Some("https://flwme.kiyt.io")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize() {
        assert_eq!(normalize_email("  Alice@Example.COM ").as_deref(), Some("alice@example.com"));
        assert!(normalize_email("not-an-email").is_none());
        assert!(normalize_email("a@b").is_none());
    }
    #[test]
    fn hashing() {
        let hash = password_hash("validpassphrase123").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(password_matches("validpassphrase123", &hash));
        assert!(!password_matches("wrongpassphrase123", &hash));
    }
    #[test]
    fn sessions_are_random_and_hashed() {
        let (a, hash) = make_session();
        let (b, _) = make_session();
        assert_ne!(a, b);
        assert_eq!(hash_token(&a).unwrap(), hash);
        assert_ne!(a.as_bytes(), hash.as_slice());
        assert!(hash_token("untrusted").is_none());
    }
    #[test]
    fn origin_is_required() {
        let headers = HeaderMap::new();
        assert!(!valid_origin(&headers));
    }
}
