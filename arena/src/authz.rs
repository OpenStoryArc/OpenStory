use crate::naming;

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum AuthzDecision {
    Allow,
    LoginRedirect,
    Deny,
}

/// `session_user`: username from a verified session cookie, if any.
/// `host`: the Host header Caddy forwarded. `base`: e.g. "arena.openstory.work".
#[allow(dead_code)]
pub fn authorize_host(session_user: Option<&str>, host: &str, base: &str) -> AuthzDecision {
    use AuthzDecision::*;

    // Strip :port suffix from host
    let host_without_port = host.split(':').next().unwrap_or(host);

    // Exact match against base → Allow
    if host_without_port == base {
        return Allow;
    }

    // Try to strip the base suffix
    let suffix = format!(".{base}");
    let Some(label) = host_without_port.strip_suffix(&suffix) else {
        // Host not under base
        return Deny;
    };

    // Label must be a single label (no '.' inside)
    if label.contains('.') {
        return Deny;
    }

    // Strip optional "-story" suffix
    let username_candidate = if let Some(stripped) = label.strip_suffix("-story") {
        stripped
    } else {
        label
    };

    // Validate the username
    if naming::validate_username(username_candidate).is_err() {
        return Deny;
    }

    // Compare with session_user
    match session_user {
        Some(user) if user == username_candidate => Allow,
        Some(_) => Deny,
        None => LoginRedirect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const BASE: &str = "arena.openstory.work";

    #[test]
    fn authz_decision_table() {
        use AuthzDecision::*;
        let cases: &[(Option<&str>, &str, AuthzDecision)] = &[
            (None, "arena.openstory.work", Allow),
            (Some("katie"), "arena.openstory.work", Allow),
            (Some("katie"), "katie.arena.openstory.work", Allow),
            (Some("katie"), "katie-story.arena.openstory.work", Allow),
            (Some("katie"), "katie.arena.openstory.work:443", Allow),
            (None, "katie.arena.openstory.work", LoginRedirect),
            (Some("bob"), "katie.arena.openstory.work", Deny),
            (Some("bob"), "katie-story.arena.openstory.work", Deny),
            (Some("katie"), "evil.example.com", Deny),
            (Some("katie"), "deep.katie.arena.openstory.work", Deny),
            (Some("katie"), "-story.arena.openstory.work", Deny),
        ];
        for (user, host, want) in cases {
            assert_eq!(
                &authorize_host(*user, host, BASE),
                want,
                "user={user:?} host={host}"
            );
        }
    }
}
