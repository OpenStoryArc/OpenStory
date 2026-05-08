//! Server configuration — loaded from config.toml, overridable by CLI flags.

use std::path::Path;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Server role — determines which subsystems start.
///
/// - `Full`: watcher + consumer + API (default, current behavior)
/// - `Publisher`: watcher only — publishes events to NATS, no local store
///   or API. (Pre-2026-04 this also exposed a `/hooks` HTTP endpoint;
///   that's been retired — only `/health` remains on this role.)
/// - `Consumer`: subscribes from NATS, runs ingest + API, no watcher
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Full,
    Publisher,
    Consumer,
}

/// Persistence backend selection. Defaults to SQLite — the in-process,
/// zero-dependency option that ships with every build. Switch to `Mongo`
/// for distributed deployments where multiple consumers want to share
/// state across hosts.
///
/// `Mongo` requires the `open-story-store/mongo` feature to be enabled
/// at build time. If the feature is off, selecting `Mongo` will error
/// clearly at boot rather than silently falling back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DataBackend {
    #[default]
    Sqlite,
    Mongo,
}

impl fmt::Display for DataBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataBackend::Sqlite => write!(f, "sqlite"),
            DataBackend::Mongo => write!(f, "mongo"),
        }
    }
}

impl FromStr for DataBackend {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sqlite" => Ok(DataBackend::Sqlite),
            "mongo" | "mongodb" => Ok(DataBackend::Mongo),
            _ => Err(format!(
                "invalid data_backend '{}': expected 'sqlite' or 'mongo'",
                s
            )),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Full => write!(f, "full"),
            Role::Publisher => write!(f, "publisher"),
            Role::Consumer => write!(f, "consumer"),
        }
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(Role::Full),
            "publisher" => Ok(Role::Publisher),
            "consumer" => Ok(Role::Consumer),
            _ => Err(format!("invalid role '{}': expected full, publisher, or consumer", s)),
        }
    }
}

/// Person identity in OpenStory's directory model — a sovereign human
/// who owns a fleet of principals (devices, agents). Distinct from the
/// OS-level `user` field on CloudEvents: `user` records who was at the
/// keyboard; `person_id` records the OpenStory directory entity that
/// owns this OpenStory instance.
///
/// Configured under `[person]` in config.toml. On first boot (no
/// `[person]` section), one is auto-created with a generated UUID and
/// detected (host, user) matchers — see `state.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Person {
    /// UUID — generated at first boot if not provided.
    pub id: String,
    /// Friendly name shown in the UI ("You", "Max", etc.).
    pub display_name: String,
    /// Optional email address. Empty string if unknown.
    #[serde(default)]
    pub email: String,
    /// Principals in this person's fleet. Each principal owns events
    /// matched by its matchers.
    #[serde(default)]
    pub principals: Vec<Principal>,
}

/// Principal — a device or agent in a person's fleet. Each principal
/// has matcher rules that determine which events get tagged with its id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Principal {
    /// UUID for this principal.
    pub id: String,
    /// Friendly display name ("MacBook (Claude Code)", "Hetzner (Bobby)").
    pub display_name: String,
    /// Source-matching rules. First matching principal wins (precedence
    /// is the order in `Person::principals`).
    #[serde(default)]
    pub matchers: PrincipalMatchers,
}

/// Source-matching rules for a Principal. Every `Some` field must match
/// the source signal; `None` fields are wildcards. An entirely-empty
/// PrincipalMatchers matches everything (useful for a catch-all default).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrincipalMatchers {
    /// Match the agent platform field on the CloudEvent
    /// (e.g. "claude-code", "pi-mono", "hermes").
    #[serde(default)]
    pub agent: Option<String>,
    /// Match the OS hostname.
    #[serde(default)]
    pub host: Option<String>,
    /// Match the OS user.
    #[serde(default)]
    pub user: Option<String>,
    /// Match the source path against this regex (e.g. a watch dir path).
    #[serde(default)]
    pub watch_dir_pattern: Option<String>,
}

/// Server configuration with sensible defaults.
///
/// Load order: defaults → config.toml → CLI flags → env vars.
/// Each layer overrides the previous.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // ── server ──
    /// Host to bind to. Default: 127.0.0.1 (localhost only — prevents LAN exposure).
    pub host: String,
    /// Port to listen on.
    pub port: u16,
    /// Server role: full (default), publisher, or consumer.
    pub role: Role,

    // ── security ──
    /// Bearer token for API authentication. Empty = no auth (pass-through).
    pub api_token: String,
    /// SQLCipher encryption key for the database. Empty = unencrypted.
    pub db_key: String,
    /// Allowed CORS origins. Empty = allow localhost defaults only.
    pub allowed_origins: Vec<String>,

    // ── storage ──
    /// Directory for persisted data (SQLite DB, JSONL, plans).
    pub data_dir: String,
    /// Directory to watch for Claude Code transcript files.
    pub watch_dir: String,
    /// Directory to watch for pi-mono session files. Empty = disabled.
    pub pi_watch_dir: String,
    /// Directory to watch for Hermes Agent plugin JSONL files. Empty = disabled.
    /// The hermes-openstory plugin writes per-session JSONL here; the watcher
    /// auto-detects the format via `envelope.source == "hermes"`.
    pub hermes_watch_dir: String,
    /// Persistence backend: "sqlite" (default) or "mongo".
    /// `mongo` requires building with `--features open-story-store/mongo`.
    pub data_backend: DataBackend,
    /// MongoDB connection URI. Used only when `data_backend = "mongo"`.
    /// Example: `mongodb://localhost:27017` or `mongodb://user:pass@host/db?replicaSet=...`.
    pub mongo_uri: String,
    /// MongoDB database name. Used only when `data_backend = "mongo"`.
    pub mongo_db: String,

    // ── bus ──
    /// NATS server URL for event bus.
    pub nats_url: String,

    // ── tuning ──
    /// Maximum records sent in the WebSocket initial_state handshake.
    /// Higher values give more history on connect but increase payload size.
    pub max_initial_records: usize,
    /// How far back (in hours) the watcher's startup backfill scans existing
    /// JSONL files in `watch_dir`. Files whose mtime is older than this window
    /// are skipped — they're treated as historical noise that the user can
    /// query via `/api/sessions` from the EventStore but doesn't need to
    /// re-stream live. Set to `0` to disable the filter (load every JSONL
    /// the watcher sees, regardless of age) — useful for tests with static
    /// fixture data.
    pub watch_backfill_hours: u64,
    /// Payload size (bytes) above which tool outputs are truncated in WireRecords.
    /// Full content available via the /content endpoint.
    pub truncation_threshold: usize,
    /// Seconds of inactivity before a session is marked "stale".
    pub stale_threshold_secs: i64,
    /// Size of the broadcast channel for WebSocket subscribers.
    pub broadcast_channel_size: usize,
    /// Enable Prometheus metrics endpoint at /metrics. Default: false.
    pub metrics_enabled: bool,
    /// Auto-delete sessions older than this many days on boot. 0 = no cleanup.
    pub retention_days: u32,

    // ── person ──
    /// Person + fleet identity for stamping events with `person_id` and
    /// `principal_id`. None at parse time triggers first-boot bootstrap
    /// (see state.rs) which creates a default Person with detected
    /// (host, user) matchers and persists back to config.toml.
    #[serde(default)]
    pub person: Option<Person>,
}

/// Auto-detect the appropriate bind address.
///
/// Containers and WSL should bind to all interfaces (0.0.0.0) so they're
/// reachable from the host/network. Local dev defaults to localhost for safety.
///
/// Detection order:
/// 1. Container: `/.dockerenv` exists, or `container` env var set
/// 2. WSL: `WSL_DISTRO_NAME` env var set
/// 3. Otherwise: `127.0.0.1`
fn auto_detect_host() -> String {
    // Container detection
    if std::path::Path::new("/.dockerenv").exists()
        || std::env::var("container").is_ok()
    {
        return "0.0.0.0".to_string();
    }

    // WSL detection
    if std::env::var("WSL_DISTRO_NAME").is_ok() {
        return "0.0.0.0".to_string();
    }

    // Safe default: localhost only
    "127.0.0.1".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: auto_detect_host(),
            port: 3002,
            role: Role::Full,
            api_token: String::new(),
            db_key: String::new(),
            allowed_origins: Vec::new(),
            data_dir: "./data".to_string(),
            watch_dir: String::new(), // resolved at runtime
            pi_watch_dir: String::new(), // disabled by default
            hermes_watch_dir: String::new(), // disabled by default
            data_backend: DataBackend::Sqlite,
            mongo_uri: "mongodb://localhost:27017".to_string(),
            mongo_db: "openstory".to_string(),
            nats_url: "nats://localhost:4222".to_string(),
            max_initial_records: 2000,
            watch_backfill_hours: 24,
            truncation_threshold: 100_000,
            stale_threshold_secs: 300,
            broadcast_channel_size: 256,
            metrics_enabled: false,
            retention_days: 0,
            person: None,
        }
    }
}

impl Config {
    /// Load config from a TOML file, falling back to defaults for missing fields.
    pub fn from_file(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("  \x1b[33mWarning: invalid config.toml: {e}\x1b[0m");
                    eprintln!("  \x1b[33mUsing defaults\x1b[0m");
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("  \x1b[33mWarning: cannot read config.toml: {e}\x1b[0m");
                Self::default()
            }
        }
    }

    /// Auto-create a `[person]` section if missing. On first boot (no
    /// `[person]` in config.toml), generates a Person with a fresh UUID
    /// and one default Principal whose matchers reflect the detected
    /// `(host, user)` of this process — same resolution as
    /// [`open_story_core::host::host`] / [`open_story_core::user::user`].
    /// Persists the updated config back to `path` so the user's edits
    /// survive restarts.
    ///
    /// Idempotent — does nothing if `self.person` is already set.
    /// Persistence failure is non-fatal: the in-memory bootstrap still
    /// works for this session, and the next boot will retry.
    pub fn ensure_person_bootstrap(&mut self, path: &Path) {
        if self.person.is_some() {
            return;
        }
        let host = open_story_core::host::host();
        let user = open_story_core::user::user();
        let person = Person {
            id: Uuid::new_v4().to_string(),
            display_name: "You".to_string(),
            email: String::new(),
            principals: vec![Principal {
                id: Uuid::new_v4().to_string(),
                display_name: host.to_string(),
                matchers: PrincipalMatchers {
                    agent: None,
                    host: Some(host.to_string()),
                    user: Some(user.to_string()),
                    watch_dir_pattern: None,
                },
            }],
        };
        self.person = Some(person);

        match self.write_to(path) {
            Ok(()) => {
                eprintln!(
                    "  \x1b[32mFirst boot: auto-created [person] in {} with detected (host={host}, user={user})\x1b[0m",
                    path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "  \x1b[33mWarning: bootstrapped [person] in memory but failed to persist to {}: {e}\x1b[0m",
                    path.display()
                );
            }
        }
    }

    /// Serialize the current Config back to disk as TOML. Used by
    /// `ensure_person_bootstrap` to persist the auto-created [person]
    /// section. Overwrites the file at `path`.
    fn write_to(&self, path: &Path) -> std::io::Result<()> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, contents)
    }

    /// Write a default config file with comments explaining each field.
    pub fn write_default(path: &Path) -> std::io::Result<()> {
        let contents = r#"# Open Story configuration
# Place this file at {data_dir}/config.toml
# CLI flags and environment variables override these values.

# ── Server ──
# host = "127.0.0.1"
# port = 3002

# ── Security ──
# Bearer token for API authentication. Empty = no auth.
# api_token = ""
# SQLCipher encryption key for the database. Empty = unencrypted.
# db_key = ""
# Allowed CORS origins. Empty = localhost defaults only.
# allowed_origins = ["http://localhost:5173", "http://localhost:3002"]

# ── Bus ──
# nats_url = "nats://localhost:4222"

# ── Tuning ──
# Max records in the WebSocket initial_state handshake.
# max_initial_records = 2000

# How far back (hours) the watcher's startup backfill scans existing
# JSONL files in `watch_dir`. Files whose mtime is older than this window
# are skipped. Set to 0 to disable the filter (load every JSONL the
# watcher sees, regardless of age) — useful for tests with static fixture data.
# watch_backfill_hours = 24

# Payload size (bytes) above which tool outputs are truncated.
# Full content available via /api/sessions/{id}/events/{eid}/content.
# truncation_threshold = 100000

# Seconds of inactivity before a session shows as "stale".
# stale_threshold_secs = 300

# Broadcast channel size for WebSocket subscribers.
# broadcast_channel_size = 256

# ── Observability ──
# Enable Prometheus metrics endpoint at /metrics.
# metrics_enabled = false

# ── Lifecycle ──
# Auto-delete sessions older than this many days on boot. 0 = no cleanup.
# retention_days = 0

# ── Person ──
# OpenStory's identity model — your sovereign self plus the fleet of
# devices/agents that act on your behalf. On first boot this section
# is auto-filled with a generated UUID and detected (host, user)
# matchers so the system Just Works. Edit later to add more principals
# (e.g. an agent running on a remote VPS) or rename existing ones.
#
# [person]
# id = "auto-generated-uuid"
# display_name = "You"
# email = ""
#
# [[person.principals]]
# id = "auto-generated-uuid"
# display_name = "MacBook (Claude Code)"
# [person.principals.matchers]
# host = "Maxs-Air"        # OS hostname
# user = "max"             # OS username
# # agent = "claude-code"  # optional — restrict to one agent platform
# # watch_dir_pattern = "" # optional — regex on source path
"#;
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn auto_detect_host_returns_localhost_on_desktop() {
        // On a normal desktop (no container, no WSL), should return 127.0.0.1
        // This test runs on the dev machine, so it validates the default path.
        // In CI containers it may return 0.0.0.0 — that's correct too.
        let host = auto_detect_host();
        assert!(
            host == "127.0.0.1" || host == "0.0.0.0",
            "auto_detect_host should return a valid bind address, got: {host}"
        );
    }

    #[test]
    fn auto_detect_host_is_used_in_default_config() {
        let config = Config::default();
        // Host should match auto_detect_host result
        assert_eq!(config.host, auto_detect_host());
    }

    #[test]
    fn role_from_str_parses_all_variants() {
        assert_eq!("full".parse::<Role>().unwrap(), Role::Full);
        assert_eq!("publisher".parse::<Role>().unwrap(), Role::Publisher);
        assert_eq!("consumer".parse::<Role>().unwrap(), Role::Consumer);
        assert_eq!("FULL".parse::<Role>().unwrap(), Role::Full);
        assert_eq!("Publisher".parse::<Role>().unwrap(), Role::Publisher);
        assert!("invalid".parse::<Role>().is_err());
    }

    #[test]
    fn role_display_round_trips() {
        for role in [Role::Full, Role::Publisher, Role::Consumer] {
            let s = role.to_string();
            let parsed: Role = s.parse().unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn default_config_has_sensible_values() {
        let config = Config::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3002);
        assert_eq!(config.role, Role::Full);
        assert_eq!(config.api_token, "");
        assert!(config.allowed_origins.is_empty());
        assert_eq!(config.max_initial_records, 2000);
        assert_eq!(config.watch_backfill_hours, 24);
        assert_eq!(config.truncation_threshold, 100_000);
        assert_eq!(config.stale_threshold_secs, 300);
        assert_eq!(config.broadcast_channel_size, 256);
        assert!(!config.metrics_enabled);
        assert!(config.person.is_none(), "person defaults to None — first-boot bootstrap fills it");
    }

    #[test]
    fn from_file_returns_defaults_when_missing() {
        let config = Config::from_file(Path::new("/nonexistent/config.toml"));
        assert_eq!(config.port, 3002);
    }

    #[test]
    fn from_file_parses_partial_toml() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "port = 8080\nmax_initial_records = 1000\n").unwrap();

        let config = Config::from_file(&path);
        assert_eq!(config.port, 8080);
        assert_eq!(config.max_initial_records, 1000);
        // Unset fields get defaults
        assert_eq!(config.watch_backfill_hours, 24);
        assert_eq!(config.truncation_threshold, 100_000);
    }

    #[test]
    fn from_file_handles_invalid_toml() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "this is not valid toml {{{{").unwrap();

        let config = Config::from_file(&path);
        // Falls back to defaults
        assert_eq!(config.port, 3002);
    }

    #[test]
    fn write_default_creates_commented_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        Config::write_default(&path).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("max_initial_records"));
        assert!(contents.contains("truncation_threshold"));
        // All lines should be comments or blank (no active config)
        for line in contents.lines() {
            assert!(
                line.is_empty() || line.starts_with('#'),
                "default config should be all comments, found: {line}"
            );
        }
    }

    #[test]
    fn full_config_round_trips() {
        let config = Config {
            host: "127.0.0.1".into(),
            port: 9999,
            role: Role::Full,
            api_token: "test-token".into(),
            db_key: "my-secret-key".into(),
            allowed_origins: vec!["http://localhost:5173".into()],
            data_dir: "/tmp/data".into(),
            watch_dir: "/tmp/watch".into(),
            pi_watch_dir: String::new(),
            hermes_watch_dir: String::new(),
            data_backend: DataBackend::Sqlite,
            mongo_uri: "mongodb://localhost:27017".into(),
            mongo_db: "openstory".into(),
            nats_url: "nats://custom:4222".into(),
            max_initial_records: 100,
            watch_backfill_hours: 48,
            truncation_threshold: 4000,
            stale_threshold_secs: 600,
            broadcast_channel_size: 512,
            metrics_enabled: true,
            retention_days: 90,
            person: None,
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.port, 9999);
        assert_eq!(parsed.max_initial_records, 100);
        assert_eq!(parsed.stale_threshold_secs, 600);
        assert_eq!(parsed.api_token, "test-token");
        assert_eq!(parsed.allowed_origins, vec!["http://localhost:5173"]);
        assert!(parsed.metrics_enabled);
    }

    #[test]
    fn security_fields_default_to_permissive() {
        let config: Config = toml::from_str("port = 8080").unwrap();
        assert_eq!(config.api_token, "", "api_token should default to empty (no auth)");
        assert!(config.allowed_origins.is_empty(), "allowed_origins should default to empty");
        assert!(!config.metrics_enabled);
    }

    // ── person section round-trip ──────────────────────────────────────

    #[test]
    fn person_section_defaults_to_none_when_absent() {
        let config: Config = toml::from_str("port = 8080").unwrap();
        assert!(
            config.person.is_none(),
            "missing [person] section should yield None, got: {:?}",
            config.person
        );
    }

    #[test]
    fn person_section_round_trips_with_principals() {
        let toml_input = r#"
port = 3002

[person]
id = "person-uuid-001"
display_name = "Max"
email = "max@example.test"

[[person.principals]]
id = "principal-uuid-laptop"
display_name = "MacBook (Claude Code)"
[person.principals.matchers]
host = "Maxs-Air"
user = "max"
agent = "claude-code"

[[person.principals]]
id = "principal-uuid-bobby"
display_name = "Hetzner (Bobby)"
[person.principals.matchers]
agent = "openclaw"
"#;
        let config: Config = toml::from_str(toml_input).unwrap();
        let person = config.person.clone().expect("[person] section should parse");
        assert_eq!(person.id, "person-uuid-001");
        assert_eq!(person.display_name, "Max");
        assert_eq!(person.email, "max@example.test");
        assert_eq!(person.principals.len(), 2);

        let laptop = &person.principals[0];
        assert_eq!(laptop.display_name, "MacBook (Claude Code)");
        assert_eq!(laptop.matchers.host.as_deref(), Some("Maxs-Air"));
        assert_eq!(laptop.matchers.user.as_deref(), Some("max"));
        assert_eq!(laptop.matchers.agent.as_deref(), Some("claude-code"));
        assert!(laptop.matchers.watch_dir_pattern.is_none());

        let bobby = &person.principals[1];
        assert_eq!(bobby.display_name, "Hetzner (Bobby)");
        assert_eq!(bobby.matchers.agent.as_deref(), Some("openclaw"));
        assert!(bobby.matchers.host.is_none());

        // Round-trip: serialize what we parsed and re-parse it. Equality
        // of parsed forms is what matters (TOML formatting may vary).
        let serialized = toml::to_string(&config).unwrap();
        let reparsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config.person, reparsed.person);
    }

    // ── ensure_person_bootstrap ─────────────────────────────────────────

    #[test]
    fn ensure_person_bootstrap_creates_person_when_missing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut config = Config::default();
        assert!(config.person.is_none());

        config.ensure_person_bootstrap(&path);

        let person = config.person.as_ref().expect("bootstrap must populate person");
        assert!(!person.id.is_empty(), "person.id must be populated");
        assert_eq!(person.display_name, "You");
        assert_eq!(person.principals.len(), 1, "exactly one default principal");
        let principal = &person.principals[0];
        assert!(!principal.id.is_empty(), "principal.id must be populated");
        // Detected (host, user) — won't match anything specific but must be Some.
        assert!(principal.matchers.host.is_some(), "host matcher prefilled");
        assert!(principal.matchers.user.is_some(), "user matcher prefilled");
        assert!(principal.matchers.agent.is_none(), "agent left wildcard");
        assert!(principal.matchers.watch_dir_pattern.is_none());
    }

    #[test]
    fn ensure_person_bootstrap_persists_to_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut config = Config::default();
        config.ensure_person_bootstrap(&path);

        assert!(path.exists(), "bootstrap should write config.toml");
        let written: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written.person, config.person,
            "persisted person should round-trip identically"
        );
    }

    #[test]
    fn ensure_person_bootstrap_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut config = Config::default();
        config.ensure_person_bootstrap(&path);
        let first = config.person.clone().unwrap();

        // Second call must be a no-op — same UUIDs, same matchers.
        config.ensure_person_bootstrap(&path);
        let second = config.person.clone().unwrap();
        assert_eq!(first, second, "second bootstrap must not mutate");
    }

    #[test]
    fn ensure_person_bootstrap_preserves_existing_config_fields() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut config = Config::default();
        config.port = 9999;
        config.api_token = "test-token".into();
        config.ensure_person_bootstrap(&path);

        let written: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.port, 9999, "port survives the rewrite");
        assert_eq!(written.api_token, "test-token", "api_token survives");
        assert!(written.person.is_some(), "person added");
    }

    #[test]
    fn person_with_empty_matchers_parses() {
        // A catch-all default principal — empty matchers match everything.
        let toml_input = r#"
[person]
id = "p-1"
display_name = "You"

[[person.principals]]
id = "k-1"
display_name = "default"
matchers = {}
"#;
        let config: Config = toml::from_str(toml_input).unwrap();
        let person = config.person.unwrap();
        assert_eq!(person.email, "", "email defaults to empty when omitted");
        let p = &person.principals[0];
        assert!(p.matchers.agent.is_none());
        assert!(p.matchers.host.is_none());
        assert!(p.matchers.user.is_none());
        assert!(p.matchers.watch_dir_pattern.is_none());
    }
}
