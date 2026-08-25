use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;

use crate::auth::RateLimiter;
use crate::db::Db;
use crate::driver::SandboxDriver;
use crate::keys::KeyMinter;

const DEFAULT_LITELLM_URL: &str = "http://arena-litellm:4000";
const DEFAULT_LISTEN: &str = "0.0.0.0:8080";

/// Minimum decoded key length required by `cookie::Key` for HMAC signing.
const MIN_COOKIE_KEY_BYTES: usize = 64;

/// Static configuration for the arena HTTP surface, loaded from `ARENA_*`
/// env vars. See `ArenaConfig::from_env`.
#[derive(Clone)]
pub struct ArenaConfig {
    pub base_domain: String,
    pub cookie_key: Key,
    pub docker_runtime: Option<String>,
    pub litellm_url: String,
    pub listen: String,
    pub db_path: PathBuf,
}

/// Decode a hex string to bytes. Operates on the raw byte slice (never
/// indexes the `&str` by offset) so a non-ASCII value can't panic on a
/// "byte index is not a char boundary" — it just fails to parse as hex,
/// same as any other malformed input.
fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let bytes = s.trim().as_bytes();
    if !bytes.len().is_multiple_of(2) {
        bail!(
            "ARENA_COOKIE_KEY must have an even number of hex digits, got {} bytes",
            bytes.len()
        );
    }
    bytes
        .chunks(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16);
            let lo = (pair[1] as char).to_digit(16);
            match (hi, lo) {
                (Some(hi), Some(lo)) => Ok(((hi as u8) << 4) | lo as u8),
                _ => Err(anyhow!(
                    "ARENA_COOKIE_KEY contains a non-hex byte pair: {:?}",
                    String::from_utf8_lossy(pair)
                )),
            }
        })
        .collect()
}

impl ArenaConfig {
    pub fn from_env() -> Result<Self> {
        let base_domain = std::env::var("ARENA_BASE_DOMAIN")
            .context("ARENA_BASE_DOMAIN is required")?;

        let cookie_key_hex = std::env::var("ARENA_COOKIE_KEY")
            .context("ARENA_COOKIE_KEY is required")?;
        let cookie_key_bytes = decode_hex(&cookie_key_hex)?;
        if cookie_key_bytes.len() < MIN_COOKIE_KEY_BYTES {
            bail!(
                "ARENA_COOKIE_KEY must decode to at least {MIN_COOKIE_KEY_BYTES} bytes, got {}",
                cookie_key_bytes.len()
            );
        }
        let cookie_key = Key::from(&cookie_key_bytes);

        let docker_runtime = std::env::var("ARENA_DOCKER_RUNTIME").ok();

        let litellm_url =
            std::env::var("ARENA_LITELLM_URL").unwrap_or_else(|_| DEFAULT_LITELLM_URL.to_string());

        let listen = std::env::var("ARENA_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_string());

        let db_path = std::env::var("ARENA_DB")
            .context("ARENA_DB is required")
            .map(PathBuf::from)?;

        Ok(ArenaConfig {
            base_domain,
            cookie_key,
            docker_runtime,
            litellm_url,
            listen,
            db_path,
        })
    }
}

/// Shared application state handed to every axum handler. Cloned per-request
/// (cheap: everything inside is an `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub driver: Arc<dyn SandboxDriver>,
    pub minter: Arc<dyn KeyMinter>,
    pub cfg: Arc<ArenaConfig>,
    pub limiter: Arc<Mutex<RateLimiter>>,
    /// Second, IP-keyed rate limiter for `/register` (20/60s), independent of
    /// the per-username limiter above. See the trust-precondition comment on
    /// its use in `routes::post_register` — the IP key is only meaningful
    /// once Caddy overwrites `X-Forwarded-For` unconditionally.
    pub ip_limiter: Arc<Mutex<RateLimiter>>,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cfg.cookie_key.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex64() -> String {
        "aa".repeat(64)
    }

    // `std::env::set_var`/`remove_var` are process-global, and `cargo test`
    // runs tests in parallel threads by default. Both env-var scenarios are
    // combined into a single test (rather than split across two `#[test]`
    // functions) so they can't race each other's environment mutations.
    #[test]
    fn from_env_validates_cookie_key_and_applies_defaults() {
        std::env::set_var("ARENA_BASE_DOMAIN", "arena.test");
        std::env::set_var("ARENA_COOKIE_KEY", "aa".repeat(10));
        std::env::set_var("ARENA_DB", "/tmp/arena-test.db");
        let short_key_result = ArenaConfig::from_env();

        std::env::set_var("ARENA_COOKIE_KEY", hex64());
        std::env::remove_var("ARENA_DOCKER_RUNTIME");
        std::env::remove_var("ARENA_LITELLM_URL");
        std::env::remove_var("ARENA_LISTEN");
        let cfg = ArenaConfig::from_env();

        std::env::remove_var("ARENA_BASE_DOMAIN");
        std::env::remove_var("ARENA_COOKIE_KEY");
        std::env::remove_var("ARENA_DB");

        assert!(
            short_key_result.is_err(),
            "a 10-byte decoded key must be rejected"
        );

        let cfg = cfg.unwrap();
        assert_eq!(cfg.base_domain, "arena.test");
        assert_eq!(cfg.docker_runtime, None);
        assert_eq!(cfg.litellm_url, DEFAULT_LITELLM_URL);
        assert_eq!(cfg.listen, DEFAULT_LISTEN);
    }

    #[test]
    fn decode_hex_rejects_non_ascii_input_without_panicking() {
        // "aée" is 4 bytes (even length), but byte offset 2 lands in the
        // middle of "é"'s 2-byte UTF-8 encoding. The old `&s[i..i+2]`
        // byte-slicing implementation panicked with "byte index 2 is not
        // a char boundary" on input like this; the byte-chunked rewrite
        // must return an `Err` instead.
        let result = decode_hex("aée");
        assert!(result.is_err(), "non-hex/non-ASCII input must error, not panic");
    }
}
