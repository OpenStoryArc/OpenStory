//! Server configuration — loaded from config.toml, overridable by CLI flags.

use std::path::Path;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Server role — determines which subsystems start.
///
/// - `Full`: watcher + consumer + API (default, current behavior)
/// - `Publisher`: watcher + hooks server, publishes to NATS, no local store
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
}
