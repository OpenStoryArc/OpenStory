//! Minimal HTML pages, rendered with plain `str::replace` templating.
//! No template crate — three tiny static files, one placeholder. YAGNI.

const LANDING: &str = include_str!("assets/landing.html");
const LOGIN: &str = include_str!("assets/login.html");
const REGISTER: &str = include_str!("assets/register.html");

/// Landing page for a logged-in user, with `{{username}}` interpolated.
pub fn landing_page(username: &str) -> String {
    LANDING.replace("{{username}}", username)
}

/// Login form.
pub fn login_page() -> String {
    LOGIN.to_string()
}

/// Registration form (join code + username + password), with the
/// retained-JSONL consent notice near the submit button.
pub fn register_page() -> String {
    REGISTER.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landing_page_interpolates_username() {
        let html = landing_page("katie");
        assert!(html.contains("Welcome, katie"));
        assert!(!html.contains("{{username}}"));
    }

    #[test]
    fn register_page_carries_the_retention_consent_sentence() {
        let html = register_page();
        assert!(html.contains(
            "Session history from this event may be retained by the organizer."
        ));
    }

    #[test]
    fn login_page_links_to_register() {
        let html = login_page();
        assert!(html.contains("/register"));
    }
}
