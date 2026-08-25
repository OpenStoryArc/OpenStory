use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct EventManifest {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub join_code: Option<String>,
    #[serde(default)]
    pub roster: Option<Vec<String>>,
    pub ttl_hours: u64,
    pub budget_usd: f64,
    #[serde(default = "default_true")]
    pub retain_jsonl: bool,
}

#[allow(dead_code)]
fn default_true() -> bool { true }

#[allow(dead_code)]
impl EventManifest {
    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        let m: EventManifest = toml::from_str(s)?;
        m.validate().map_err(|e| anyhow::anyhow!(e))?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || !self.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(format!("event name must be [a-z0-9-]+, got {:?}", self.name));
        }
        match (&self.join_code, &self.roster) {
            (Some(_), Some(_)) => return Err("set join_code OR roster, not both".into()),
            (None, None) => return Err("one of join_code or roster is required".into()),
            _ => {}
        }
        if self.ttl_hours == 0 { return Err("ttl_hours must be >= 1".into()); }
        if self.budget_usd <= 0.0 { return Err("budget_usd must be > 0".into()); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
        name = "uva-fall"
        image = "ghcr.io/openstoryarc/arena-sandbox:2026-09-01"
        join_code = "uva-2026"
        ttl_hours = 6
        budget_usd = 5.0
    "#;

    #[test]
    fn parses_a_join_code_event_with_defaults() {
        let m = EventManifest::from_toml(GOOD).unwrap();
        assert_eq!(m.name, "uva-fall");
        assert_eq!(m.join_code.as_deref(), Some("uva-2026"));
        assert!(m.roster.is_none());
        assert!(m.retain_jsonl, "retain_jsonl defaults to true");
    }

    #[test]
    fn parses_a_roster_event() {
        let s = GOOD.replace(r#"join_code = "uva-2026""#, r#"roster = ["katie", "engineer-a"]"#);
        let m = EventManifest::from_toml(&s).unwrap();
        assert_eq!(m.roster.as_deref(), Some(&["katie".to_string(), "engineer-a".to_string()][..]));
    }

    #[test]
    fn rejects_both_join_code_and_roster() {
        let s = format!("{GOOD}\nroster = [\"katie\"]");
        assert!(EventManifest::from_toml(&s).is_err());
    }

    #[test]
    fn rejects_neither_join_code_nor_roster() {
        let s = GOOD.replace(r#"join_code = "uva-2026""#, "");
        assert!(EventManifest::from_toml(&s).is_err());
    }

    #[test]
    fn rejects_bad_event_name() {
        let s = GOOD.replace(r#"name = "uva-fall""#, r#"name = "UVA Fall!""#);
        assert!(EventManifest::from_toml(&s).is_err());
    }

    #[test]
    fn rejects_zero_ttl_and_zero_budget() {
        assert!(EventManifest::from_toml(&GOOD.replace("ttl_hours = 6", "ttl_hours = 0")).is_err());
        assert!(EventManifest::from_toml(&GOOD.replace("budget_usd = 5.0", "budget_usd = 0.0")).is_err());
    }
}
