use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use rand::Rng;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    use argon2::password_hash::PasswordHash;
    use argon2::PasswordVerifier;

    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn generate_password() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rngs::OsRng;
    (0..12)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

pub struct RateLimiter {
    max: u32,
    window: Duration,
    hits: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    pub fn new(max: u32, window: Duration) -> Self {
        RateLimiter {
            max,
            window,
            hits: HashMap::new(),
        }
    }

    pub fn check(&mut self, key: &str) -> bool {
        let now = Instant::now();

        let hits = self.hits.entry(key.to_string()).or_default();

        // Prune hits older than the window. If the window exceeds the monotonic
        // clock's history (checked_sub returns None), keep all hits.
        if let Some(cutoff) = now.checked_sub(self.window) {
            hits.retain(|&hit| hit > cutoff);
        }

        // Check if we've hit the limit
        if hits.len() < self.max as usize {
            hits.push(now);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn hash_then_verify_roundtrips_and_rejects_wrong_password() {
        let h = hash_password("brave-otter-array").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("brave-otter-array", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("brave-otter-array", "not-a-hash"));
    }

    #[test]
    fn generated_passwords_are_long_lowercase_and_distinct() {
        let a = generate_password();
        let b = generate_password();
        assert_eq!(a.len(), 12);
        assert!(a.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_ne!(a, b);
    }

    #[test]
    fn rate_limiter_allows_up_to_max_then_blocks() {
        let mut rl = RateLimiter::new(3, Duration::from_secs(60));
        assert!(rl.check("katie"));
        assert!(rl.check("katie"));
        assert!(rl.check("katie"));
        assert!(!rl.check("katie"), "4th attempt in window is blocked");
        assert!(rl.check("bob"), "other keys unaffected");
    }

    #[test]
    fn rate_limiter_with_huge_window_does_not_panic_and_still_blocks() {
        let mut rl = RateLimiter::new(1, Duration::MAX);
        assert!(rl.check("k"), "first check should allow");
        assert!(
            !rl.check("k"),
            "second check should block (hit within huge window)"
        );
    }
}
