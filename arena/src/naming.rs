#[allow(dead_code)]
pub const RESERVED: &[&str] = &["arena", "www", "api", "litellm", "hub", "caddy", "admin", "story"];

#[allow(dead_code)]
pub fn validate_username(u: &str) -> Result<(), String> {
    let ok_len = (2..=31).contains(&u.len());
    let ok_first = u.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let ok_chars = u.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !(ok_len && ok_first && ok_chars) {
        return Err("username must be 2-31 chars of [a-z0-9-], starting with a letter or digit".into());
    }
    if u.contains("--") || u.ends_with('-') {
        return Err("username must not contain '--' or end with '-'".into());
    }
    if RESERVED.contains(&u) {
        return Err(format!("{u:?} is reserved"));
    }
    if u.ends_with("-story") {
        return Err("usernames may not end in -story".into());
    }
    Ok(())
}

#[allow(dead_code)]
pub fn container_name(u: &str) -> String {
    format!("sandbox-{u}")
}

#[allow(dead_code)]
pub fn volume_name(u: &str) -> String {
    format!("arena-home-{u}")
}

#[allow(dead_code)]
pub fn network_name(u: &str) -> String {
    format!("arena-sb-{u}")
}

#[allow(dead_code)]
pub fn terminal_host(u: &str, base: &str) -> String {
    format!("{u}.{base}")
}

#[allow(dead_code)]
pub fn story_host(u: &str, base: &str) -> String {
    format!("{u}-story.{base}")
}

#[allow(dead_code)]
pub fn key_alias(event: &str, u: &str) -> String {
    format!("{event}/{u}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_usernames() {
        for u in ["katie", "engineer-a", "max2", "a1"] {
            assert!(validate_username(u).is_ok(), "{u} should be valid");
        }
    }

    #[test]
    fn rejects_reserved_story_suffix_case_and_shape() {
        let long = "a".repeat(40);
        for u in ["arena", "litellm", "admin", "bob-story", "Katie", "-bob", "b", long.as_str(), "bo b", ""] {
            assert!(validate_username(u).is_err(), "{u:?} should be rejected");
        }
    }

    #[test]
    fn derived_names_thread_the_username() {
        assert_eq!(container_name("katie"), "sandbox-katie");
        assert_eq!(volume_name("katie"), "arena-home-katie");
        assert_eq!(network_name("katie"), "arena-sb-katie");
        assert_eq!(terminal_host("katie", "arena.openstory.work"), "katie.arena.openstory.work");
        assert_eq!(story_host("katie", "arena.openstory.work"), "katie-story.arena.openstory.work");
        assert_eq!(key_alias("uva-fall", "katie"), "uva-fall/katie");
    }
}
