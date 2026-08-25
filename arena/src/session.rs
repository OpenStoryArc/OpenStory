use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use time::Duration as CookieDuration;

pub const SESSION_COOKIE: &str = "arena_session";

/// Extract the logged-in username from a verified, signed session cookie.
pub fn session_user(jar: &SignedCookieJar) -> Option<String> {
    jar.get(SESSION_COOKIE).map(|c| c.value().to_string())
}

/// Set the session cookie for `username`, scoped to `.{base_domain}` so a
/// single login covers every sandbox subdomain.
pub fn set_session(jar: SignedCookieJar, username: &str, base_domain: &str) -> SignedCookieJar {
    let cookie = Cookie::build((SESSION_COOKIE, username.to_string()))
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
}
