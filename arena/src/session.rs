use std::time::{SystemTime, UNIX_EPOCH};

use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use time::Duration as CookieDuration;

pub const SESSION_COOKIE: &str = "arena_session";

const SESSION_LIFETIME_SECS: i64 = 12 * 60 * 60;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Extract the logged-in username from a verified, signed session cookie.
///
/// The cookie's signature proves it wasn't tampered with, but the browser
/// (or a captured cookie replayed later) is trusted to honor `Max-Age` on
/// its own — belt-and-suspenders, the session's own expiry is also carried
/// in the signed value as `"{username}|{unix_expiry}"` and checked here
/// against the current time. A malformed or expired value is treated the
/// same as "no session".
pub fn session_user(jar: &SignedCookieJar) -> Option<String> {
    let cookie = jar.get(SESSION_COOKIE)?;
    let (username, expiry_str) = cookie.value().split_once('|')?;
    let expiry: i64 = expiry_str.parse().ok()?;
    if now_unix() >= expiry {
        return None;
    }
    Some(username.to_string())
}

/// Set the session cookie for `username`, scoped to `.{base_domain}` so a
/// single login covers every sandbox subdomain.
pub fn set_session(jar: SignedCookieJar, username: &str, base_domain: &str) -> SignedCookieJar {
    let expiry = now_unix() + SESSION_LIFETIME_SECS;
    let value = format!("{username}|{expiry}");
    let cookie = Cookie::build((SESSION_COOKIE, value))
        .domain(format!(".{base_domain}"))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(CookieDuration::hours(12))
        .build();
    jar.add(cookie)
}

/// Remove the session cookie (logout).
pub fn clear_session(jar: SignedCookieJar, base_domain: &str) -> SignedCookieJar {
    let cookie = Cookie::build(SESSION_COOKIE)
        .domain(format!(".{base_domain}"))
        .path("/")
        .build();
    jar.remove(cookie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_extra::extract::cookie::Key;

    fn key() -> Key {
        Key::from(&[9u8; 64])
    }

    #[test]
    fn set_then_read_session_roundtrips_username() {
        let jar = SignedCookieJar::new(key());
        let jar = set_session(jar, "katie", "arena.test");
        assert_eq!(session_user(&jar).as_deref(), Some("katie"));
    }

    #[test]
    fn clear_session_removes_the_cookie() {
        let jar = SignedCookieJar::new(key());
        let jar = set_session(jar, "katie", "arena.test");
        assert!(session_user(&jar).is_some());
        let jar = clear_session(jar, "arena.test");
        assert!(session_user(&jar).is_none());
    }

    #[test]
    fn expired_session_value_returns_none() {
        // Build a validly-signed cookie whose embedded expiry is already in
        // the past, the same way a 12h-old (or clock-skewed) real session
        // cookie would look by the time it's read.
        let past_expiry = now_unix() - 1;
        let jar = SignedCookieJar::new(key());
        let cookie = Cookie::build((SESSION_COOKIE, format!("katie|{past_expiry}")))
            .domain(".arena.test")
            .path("/")
            .build();
        let jar = jar.add(cookie);
        assert_eq!(session_user(&jar), None);
    }

    #[test]
    fn malformed_session_value_returns_none() {
        let jar = SignedCookieJar::new(key());
        let cookie = Cookie::build((SESSION_COOKIE, "no-pipe-separator-here"))
            .domain(".arena.test")
            .path("/")
            .build();
        let jar = jar.add(cookie);
        assert_eq!(session_user(&jar), None);
    }
}
