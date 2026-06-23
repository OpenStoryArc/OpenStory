//! Server configuration — loaded from config.toml, overridable by CLI flags.

use std::fmt;
use std::path::{Path, PathBuf};
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
            _ => Err(format!(
                "invalid role '{}': expected full, publisher, or consumer",
                s
            )),
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
    /// Bearer token for **admin** mutations (Phase 6.1). Separate from
    /// `api_token` so a read-only API caller cannot escalate to policy
    /// writes. Empty = no admin separation (`api_token` accepted on
    /// admin routes too, preserving single-token deployments).
    #[serde(default)]
    pub admin_token: String,
    /// Path to the nats-server conf file managed by `AccountConfigWriter`
    /// (Phase 5 boot-wire). When set AND `[person]` is configured, the
    /// server builds the writer at boot and `POST /api/admin/share-with-person`
    /// becomes functional. The operator is responsible for pointing
    /// nats-server at this same path (`nats-server -c <path>`).
    /// **Warning:** the writer owns the file — operator edits are
    /// overwritten on every share mutation. Use a separate path from any
    /// hand-managed `nats.conf`. Empty = no multi-account mode; writer
    /// stays `None` and `share-with-person` returns 503.
    #[serde(default)]
    pub nats_accounts_conf_path: String,
    /// Shell command run after the writer persists conf changes — should
    /// signal nats-server to reread its conf. Defaults to
    /// `pkill -HUP nats-server` which works for a single locally-owned
    /// nats-server. Override for managed deployments (e.g.
    /// `systemctl reload nats` or a NATS control-subject publish).
    /// Empty = no reload (the conf updates land on disk but nats-server
    /// won't see them until restart — useful for tests, dangerous in prod).
    #[serde(default = "default_nats_reload_command")]
    pub nats_reload_command: String,
    /// Phase 6.5 — which principal this OpenStory instance represents.
    /// Used by the role middleware to look up the operator's role in the
    /// `RoleDirectory`. Empty → all role-gated routes return 403
    /// (fail-closed sovereignty: no principal identity, no permission).
    #[serde(default)]
    pub local_principal_id: String,
    /// Path to the SQLite file backing `EmbeddedRoleDirectory`. Empty →
    /// no role directory; `NoopRoleDirectory` is wired in instead, which
    /// returns `None` for every lookup and therefore 403s every role-gated
    /// request. Set this *and* populate the `participants` table to
    /// enable Layer 3 permissions.
    #[serde(default)]
    pub roles_db_path: String,
    /// SQLCipher encryption key for the database. Empty = unencrypted.
    pub db_key: String,
    /// Allowed CORS origins. Empty = allow localhost defaults only.
    pub allowed_origins: Vec<String>,

    // ── storage ──
    /// Directory for persisted data (SQLite DB, JSONL, plans).
    pub data_dir: String,
    /// Directory to watch for Claude Code transcript files.
    pub claude_watch_dir: String,
    /// Directory to watch for Codex rollout JSONL files.
    pub codex_watch_dir: String,
    /// Directory to watch for Claude Code transcript files.
    /// Legacy alias used when `watch_dirs` is empty and agent-specific roots
    /// are unset.
    pub watch_dir: String,
    /// JSONL transcript roots to watch with the same reader pipeline.
    /// Empty means use `watch_dir` only.
    pub watch_dirs: Vec<String>,
    /// Directory to watch for pi-mono session files. Empty = disabled.
    pub pi_watch_dir: String,
    /// Directory to watch for Hermes Agent plugin JSONL files. Empty = disabled.
    /// The hermes-openstory plugin writes per-session JSONL here; the watcher
    /// auto-detects the format via `envelope.source == "hermes"`.
    pub hermes_watch_dir: String,
    /// Default share policy for a session with no explicit row. When unset
    /// (`None`), the effective default is *derived from whether networking is
    /// configured* — loopback-only → `shared` (the local dashboard just works
    /// out of the box), networked → `private` (sharing is opt-in, so pointing
    /// at a hub never leaks your history). Set explicitly to pin one way. See
    /// [`Config::effective_default_share_policy`]. Env override:
    /// `OPEN_STORY_DEFAULT_SHARE_POLICY`.
    #[serde(default)]
    pub default_share_policy: Option<open_story_store::event_store::SharePolicyMode>,
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
    /// Hub URL for distributed (leaf-node) streaming. Empty by default — the
    /// install is single-machine and loopback-only out of the box. Set this to
    /// a shared hub (e.g. `nats://<token>@hub-host:7422`) and the managed NATS
    /// (`--manage-nats`) launches as a JetStream leaf instead of a standalone:
    /// it federates this machine's events up to the hub and replays everyone
    /// else's down, so the local dashboard becomes a shared multi-machine view.
    /// Also settable via `OPEN_STORY_NATS_LEAF_URL`. Has no effect without
    /// `--manage-nats` (when you run your own NATS, configure leafnodes there).
    pub nats_leaf_url: String,

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
    if std::path::Path::new("/.dockerenv").exists() || std::env::var("container").is_ok() {
        return "0.0.0.0".to_string();
    }

    // WSL detection
    if std::env::var("WSL_DISTRO_NAME").is_ok() {
        return "0.0.0.0".to_string();
    }

    // Safe default: localhost only
    "127.0.0.1".to_string()
}

fn default_nats_reload_command() -> String {
    "pkill -HUP nats-server".to_string()
}

/// Whether a bind host is loopback-only (no LAN/public exposure).
fn is_loopback_host(host: &str) -> bool {
    let h = host.trim();
    h == "localhost" || h == "::1" || h.starts_with("127.")
}

fn default_home_subdir(parts: &[&str]) -> String {
    let Some(home) = std::env::var_os(home_env_var()) else {
        return String::new();
    };
    let mut path = std::path::PathBuf::from(home);
    for part in parts {
        path.push(part);
    }
    path.to_string_lossy().to_string()
}

fn home_env_var() -> &'static str {
    if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: auto_detect_host(),
            port: 3002,
            role: Role::Full,
            api_token: String::new(),
            admin_token: String::new(),
            nats_accounts_conf_path: String::new(),
            nats_reload_command: default_nats_reload_command(),
            local_principal_id: String::new(),
            roles_db_path: String::new(),
            db_key: String::new(),
            allowed_origins: Vec::new(),
            data_dir: "./data".to_string(),
            claude_watch_dir: default_home_subdir(&[".claude", "projects"]),
            codex_watch_dir: default_home_subdir(&[".codex", "sessions"]),
            watch_dir: String::new(), // resolved at runtime
            watch_dirs: Vec::new(),
            pi_watch_dir: String::new(),     // disabled by default
            hermes_watch_dir: String::new(), // disabled by default
            default_share_policy: None,
            data_backend: DataBackend::Sqlite,
            mongo_uri: "mongodb://localhost:27017".to_string(),
            mongo_db: "openstory".to_string(),
            nats_url: "nats://localhost:4222".to_string(),
            nats_leaf_url: String::new(), // no networking by default (loopback-only)
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
    /// Resolve the default share policy for sessions with no explicit row.
    ///
    /// Precedence: `OPEN_STORY_DEFAULT_SHARE_POLICY` env var → explicit
    /// `default_share_policy` in config → derived from networking. The
    /// derivation is the heart of "easy local, safe network": a loopback-only
    /// instance (no `nats_leaf_url`) defaults to `Shared` so the local
    /// dashboard just works, while configuring a hub/leaf flips the default to
    /// `Private` so pointing at a network never auto-shares your history.
    pub fn effective_default_share_policy(&self) -> open_story_store::event_store::SharePolicyMode {
        self.effective_default_share_policy_with(
            crate::admin::EnvInputs::from_env().is_federated(),
        )
    }

    /// Testable core of [`Self::effective_default_share_policy`]. `federated`
    /// is the federation-env snapshot ([`crate::admin::EnvInputs::is_federated`]),
    /// split out so the policy logic is exercised without mutating process env.
    fn effective_default_share_policy_with(
        &self,
        federated: bool,
    ) -> open_story_store::event_store::SharePolicyMode {
        use open_story_store::event_store::SharePolicyMode;
        std::env::var("OPEN_STORY_DEFAULT_SHARE_POLICY")
            .ok()
            .and_then(|v| v.parse().ok())
            .or(self.default_share_policy)
            .unwrap_or_else(|| {
                if self.is_networked_with(federated) {
                    SharePolicyMode::Private // networked: sharing is opt-in
                } else {
                    SharePolicyMode::Shared // loopback-only: local dashboard just works
                }
            })
    }

    /// True if this instance is configured to exchange data with other
    /// machines — a NATS leaf URL (`nats_leaf_url`) OR hub/peer-domain
    /// federation. This is the single definition of "networked" used by both
    /// the safe-network share default and the trusted-local admin gate; before
    /// this existed, each checked only `nats_leaf_url` and a hub/peer-domain
    /// node was wrongly treated as loopback-only.
    pub fn is_networked(&self) -> bool {
        self.is_networked_with(crate::admin::EnvInputs::from_env().is_federated())
    }

    fn is_networked_with(&self, federated: bool) -> bool {
        !self.nats_leaf_url.trim().is_empty() || federated
    }

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
    /// Idempotent — re-running is a no-op once `[person]` and
    /// `local_principal_id` are set. Persistence failure is non-fatal: the
    /// in-memory bootstrap still works for this session, and the next boot
    /// will retry.
    ///
    /// Also fills `local_principal_id` from the first principal when empty —
    /// without it the role middleware can't identify the operator and every
    /// role-gated route 403s. This is half of the frictionless local
    /// bootstrap (the other half — auto-granting Admin on a trusted-local
    /// instance — lives in `server::create_state`).
    pub fn ensure_person_bootstrap(&mut self, path: &Path) {
        let mut changed = false;

        if self.person.is_none() {
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
            changed = true;
            eprintln!(
                "  \x1b[32mFirst boot: auto-created [person] in {} with detected (host={host}, user={user})\x1b[0m",
                path.display()
            );
        }

        // Identify the local operator so role-gated routes can authorize
        // them. Defaults to this device's first principal.
        if self.local_principal_id.is_empty() {
            if let Some(p) = self.person.as_ref().and_then(|pn| pn.principals.first()) {
                self.local_principal_id = p.id.clone();
                changed = true;
            }
        }

        if changed {
            if let Err(e) = self.write_to(path) {
                eprintln!(
                    "  \x1b[33mWarning: bootstrapped identity in memory but failed to persist to {}: {e}\x1b[0m",
                    path.display()
                );
            }
        }
    }

    /// Path to the roles SQLite DB, defaulting to `{data_dir}/openstory-roles.db`
    /// when unset. A real path (vs. empty) means the role directory is the
    /// `EmbeddedRoleDirectory` — so a CLI `grant-role` is actually read back,
    /// instead of being silently ignored by the fail-closed `NoopRoleDirectory`.
    pub fn effective_roles_db_path(&self, data_dir: &Path) -> PathBuf {
        if self.roles_db_path.trim().is_empty() {
            data_dir.join("openstory-roles.db")
        } else {
            PathBuf::from(&self.roles_db_path)
        }
    }

    /// A loopback-only instance with no API auth and no networking — the local
    /// operator is implicitly trusted (it's their own machine). Drives the
    /// frictionless defaults (auto-grant Admin, default-shared). The moment
    /// any of these change (bind to a LAN/`0.0.0.0`, set an `api_token`, or
    /// configure ANY federation — a leaf URL OR hub/peer domains) the instance
    /// is no longer trusted-local and falls back to the deliberate CLI
    /// bootstrap (no ambient admin on a node that talks to other machines).
    pub fn is_trusted_local(&self) -> bool {
        self.is_trusted_local_with(crate::admin::EnvInputs::from_env().is_federated())
    }

    fn is_trusted_local_with(&self, federated: bool) -> bool {
        is_loopback_host(&self.host)
            && self.api_token.trim().is_empty()
            && !self.is_networked_with(federated)
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
# Distributed streaming: set a shared hub URL to turn the managed NATS
# (--manage-nats) into a JetStream leaf node, so this machine's sessions stream
# to a common dashboard and other machines' sessions stream back. Empty =
# single-machine, loopback-only (the default). Also via OPEN_STORY_NATS_LEAF_URL.
# See docs/deploy/distributed.md.
# nats_leaf_url = "nats://<token>@hub-host:7422"

# ── Tuning ──
# Max records in the WebSocket initial_state handshake.
# max_initial_records = 2000

# How far back (hours) the watcher's startup backfill scans existing
# JSONL files in `watch_dir`. Files whose mtime is older than this window
# are skipped. Set to 0 to disable the filter (load every JSONL the
# watcher sees, regardless of age) — useful for tests with static fixture data.
# watch_backfill_hours = 24

# JSONL transcript roots to watch with the same reader pipeline.
# Agent-specific roots are used by default. Leave watch_dirs empty to use them.
# claude_watch_dir = "/Users/me/.claude/projects"
# codex_watch_dir = "/Users/me/.codex/sessions"
#
# Explicit roots override agent-specific defaults.
# watch_dirs = ["/Users/me/.claude/projects", "/Users/me/.codex/sessions"]

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

    /// Apply wizard answers onto this config, returning the mutated copy.
    ///
    /// Pure: takes ownership of the base (loaded from an existing config.toml
    /// or `Config::default()`) and overlays only the fields the wizard asks
    /// about. Everything else — api_token, nats_url, mongo settings, etc. —
    /// is preserved, so re-running the wizard never clobbers values it didn't
    /// prompt for.
    pub fn apply_answers(mut self, a: WizardAnswers) -> Config {
        self.watch_backfill_hours = days_to_backfill_hours(a.days_history);
        self.max_initial_records = recommended_initial_records(a.days_history, self.max_initial_records);
        self.watch_dir = a.watch_dir;
        if let Some(p) = a.pi_watch_dir {
            self.pi_watch_dir = p;
        }
        if let Some(h) = a.hermes_watch_dir {
            self.hermes_watch_dir = h;
        }
        self.port = a.port;
        self.data_dir = a.data_dir;
        self
    }

    /// Write the config's *actual* values to `path` as TOML, with a short
    /// generated header. Distinct from [`Config::write_default`], which writes
    /// a fully commented template — this serializes live, chosen settings so
    /// the server reads them back on boot.
    pub fn write_values(path: &Path, config: &Config) -> std::io::Result<()> {
        let header = "# OpenStory configuration — generated by `open-story init`.\n\
                      # Edit freely; CLI flags and environment variables override these values.\n\n";
        let body = toml::to_string(config).map_err(std::io::Error::other)?;
        std::fs::write(path, format!("{header}{body}"))
    }
}

/// Typed answers collected by the `open-story init` wizard. Plain data — the
/// interactive prompting lives in the CLI crate; this is the pure boundary
/// between "what the user said" and "how it maps onto Config".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardAnswers {
    /// Days of session history to load on boot (drives `watch_backfill_hours`).
    pub days_history: u64,
    /// Claude Code transcript directory to watch.
    pub watch_dir: String,
    /// Optional pi-mono session directory (None = leave current value).
    pub pi_watch_dir: Option<String>,
    /// Optional Hermes session directory (None = leave current value).
    pub hermes_watch_dir: Option<String>,
    /// Port the server listens on.
    pub port: u16,
    /// Directory where config.toml + data live.
    pub data_dir: String,
}

/// Convert a "days of history" answer into the watcher backfill window (hours).
/// Saturates rather than overflowing on absurd inputs.
pub fn days_to_backfill_hours(days: u64) -> u64 {
    days.saturating_mul(24)
}

/// Recommend an initial-record cap for the chosen history window.
///
/// Short windows (≤ 7 days) keep whatever baseline is already set (default
/// 2000). Wider windows scale up so the UI actually shows the backfilled
/// history, capped at 10_000 so the WebSocket initial_state handshake never
/// balloons unbounded.
pub fn recommended_initial_records(days: u64, current: usize) -> usize {
    if days <= 7 {
        current
    } else {
        let scaled = days.saturating_mul(300).min(10_000) as usize;
        current.max(scaled)
    }
}

/// Parse + validate a port answer. Rejects 0 and anything not 1..=65535.
pub fn parse_port(s: &str) -> Result<u16, String> {
    let trimmed = s.trim();
    let n: u16 = trimmed
        .parse()
        .map_err(|_| format!("'{trimmed}' is not a valid port (expected 1–65535)"))?;
    if n == 0 {
        return Err("port must be between 1 and 65535".to_string());
    }
    Ok(n)
}

/// Parse + validate a days-of-history answer. Range 1..=365.
pub fn parse_days(s: &str) -> Result<u64, String> {
    let trimmed = s.trim();
    let n: u64 = trimmed
        .parse()
        .map_err(|_| format!("'{trimmed}' is not a whole number of days"))?;
    if !(1..=365).contains(&n) {
        return Err("days must be between 1 and 365".to_string());
    }
    Ok(n)
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
        assert!(config.claude_watch_dir.ends_with(".claude/projects"));
        assert!(config.codex_watch_dir.ends_with(".codex/sessions"));
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
            admin_token: "test-admin-token".into(),
            nats_accounts_conf_path: "/tmp/test-nats.conf".into(),
            nats_reload_command: "true".into(),
            local_principal_id: "test-principal".into(),
            roles_db_path: "/tmp/test-roles.db".into(),
            db_key: "my-secret-key".into(),
            allowed_origins: vec!["http://localhost:5173".into()],
            data_dir: "/tmp/data".into(),
            claude_watch_dir: "/tmp/claude".into(),
            codex_watch_dir: "/tmp/codex-sessions".into(),
            watch_dir: "/tmp/watch".into(),
            watch_dirs: vec!["/tmp/watch".into(), "/tmp/codex".into()],
            pi_watch_dir: String::new(),
            hermes_watch_dir: String::new(),
            default_share_policy: Some(open_story_store::event_store::SharePolicyMode::Private),
            data_backend: DataBackend::Sqlite,
            mongo_uri: "mongodb://localhost:27017".into(),
            mongo_db: "openstory".into(),
            nats_url: "nats://custom:4222".into(),
            nats_leaf_url: "nats://tok@hub:7422".into(),
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
        assert_eq!(parsed.nats_leaf_url, "nats://tok@hub:7422");
        assert_eq!(parsed.allowed_origins, vec!["http://localhost:5173"]);
        assert_eq!(parsed.claude_watch_dir, "/tmp/claude");
        assert_eq!(parsed.codex_watch_dir, "/tmp/codex-sessions");
        assert_eq!(parsed.watch_dirs, vec!["/tmp/watch", "/tmp/codex"]);
        assert!(parsed.metrics_enabled);
    }

    #[test]
    fn security_fields_default_to_permissive() {
        let config: Config = toml::from_str("port = 8080").unwrap();
        assert_eq!(
            config.api_token, "",
            "api_token should default to empty (no auth)"
        );
        assert!(
            config.allowed_origins.is_empty(),
            "allowed_origins should default to empty"
        );
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
    fn ensure_person_bootstrap_sets_local_principal_id() {
        // Without local_principal_id the role middleware can't identify the
        // operator → every role-gated route 403s. Bootstrap must fill it from
        // the device's first principal.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut config = Config::default();
        assert_eq!(config.local_principal_id, "");

        config.ensure_person_bootstrap(&path);

        let principal_id = config.person.as_ref().unwrap().principals[0].id.clone();
        assert_eq!(
            config.local_principal_id, principal_id,
            "local_principal_id must default to the first principal"
        );
        // Persisted, so it survives restarts.
        let written: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.local_principal_id, principal_id);
    }

    #[test]
    fn ensure_person_bootstrap_preserves_explicit_local_principal_id() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut config = Config::default();
        config.local_principal_id = "pinned-by-operator".into();
        config.ensure_person_bootstrap(&path);
        assert_eq!(
            config.local_principal_id, "pinned-by-operator",
            "an explicit local_principal_id must not be overwritten"
        );
    }

    #[test]
    fn effective_roles_db_path_defaults_under_data_dir() {
        let config = Config::default();
        assert_eq!(config.roles_db_path, "", "precondition: unset");
        assert_eq!(
            config.effective_roles_db_path(Path::new("/var/openstory")),
            Path::new("/var/openstory/openstory-roles.db"),
            "empty roles_db_path defaults under data_dir"
        );
        let pinned = Config {
            roles_db_path: "/custom/roles.db".into(),
            ..Config::default()
        };
        assert_eq!(
            pinned.effective_roles_db_path(Path::new("/var/openstory")),
            Path::new("/custom/roles.db"),
            "explicit roles_db_path wins"
        );
    }

    #[test]
    fn is_trusted_local_only_when_loopback_unauthed_and_no_hub() {
        // Fresh default: loopback host, no token, no leaf → trusted.
        assert!(Config::default().is_trusted_local());

        // Any one of these revokes trust.
        assert!(
            !Config { host: "0.0.0.0".into(), ..Config::default() }.is_trusted_local(),
            "LAN/public bind is not trusted-local"
        );
        assert!(
            !Config { api_token: "secret".into(), ..Config::default() }.is_trusted_local(),
            "an api_token means access is gated → not the trusted single-user case"
        );
        assert!(
            !Config { nats_leaf_url: "nats://hub:7422".into(), ..Config::default() }
                .is_trusted_local(),
            "configured hub → networked → not trusted-local"
        );
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

    // ── wizard: pure logic (open-story init) ──

    #[test]
    fn days_to_backfill_hours_multiplies_by_24() {
        assert_eq!(days_to_backfill_hours(7), 168);
        assert_eq!(days_to_backfill_hours(1), 24);
        assert_eq!(days_to_backfill_hours(30), 720);
    }

    #[test]
    fn days_to_backfill_hours_saturates_instead_of_overflowing() {
        assert_eq!(days_to_backfill_hours(u64::MAX), u64::MAX);
    }

    #[test]
    fn parse_days_accepts_in_range() {
        assert_eq!(parse_days("1").unwrap(), 1);
        assert_eq!(parse_days("7").unwrap(), 7);
        assert_eq!(parse_days(" 365 ").unwrap(), 365);
    }

    #[test]
    fn parse_days_rejects_out_of_range_and_nonnumeric() {
        assert!(parse_days("0").is_err());
        assert!(parse_days("366").is_err());
        assert!(parse_days("abc").is_err());
        assert!(parse_days("").is_err());
    }

    #[test]
    fn parse_port_accepts_valid_rejects_zero_and_overflow() {
        assert_eq!(parse_port("3002").unwrap(), 3002);
        assert_eq!(parse_port(" 8080 ").unwrap(), 8080);
        assert!(parse_port("0").is_err());
        assert!(parse_port("99999").is_err()); // > u16::MAX
        assert!(parse_port("notaport").is_err());
    }

    #[test]
    fn recommended_initial_records_keeps_baseline_for_small_window() {
        assert_eq!(recommended_initial_records(7, 2000), 2000);
        assert_eq!(recommended_initial_records(1, 2000), 2000);
    }

    #[test]
    fn recommended_initial_records_scales_and_caps_for_large_window() {
        let r30 = recommended_initial_records(30, 2000);
        assert!(r30 > 2000, "30-day window should scale above baseline, got {r30}");
        assert!(r30 <= 10_000, "must stay capped at 10k, got {r30}");
        assert_eq!(recommended_initial_records(365, 2000), 10_000, "wide window caps at 10k");
    }

    #[test]
    fn apply_answers_sets_expected_fields() {
        let answers = WizardAnswers {
            days_history: 14,
            watch_dir: "/home/me/.claude/projects".into(),
            pi_watch_dir: None,
            hermes_watch_dir: None,
            port: 4000,
            data_dir: "/var/openstory".into(),
        };
        let config = Config::default().apply_answers(answers);
        assert_eq!(config.watch_backfill_hours, 14 * 24);
        assert_eq!(config.watch_dir, "/home/me/.claude/projects");
        assert_eq!(config.port, 4000);
        assert_eq!(config.data_dir, "/var/openstory");
        // Untouched optional dirs stay empty (None = leave current).
        assert_eq!(config.pi_watch_dir, "");
        assert_eq!(config.hermes_watch_dir, "");
    }

    #[test]
    fn networking_is_off_by_default() {
        // Sovereignty default: a fresh install is single-machine and
        // loopback-only. Distributed streaming is strictly opt-in via a hub URL.
        assert_eq!(Config::default().nats_leaf_url, "");
    }

    #[test]
    fn effective_share_policy_is_shared_when_loopback_only() {
        use open_story_store::event_store::SharePolicyMode;
        // Easy local: a fresh, loopback-only install makes its own sessions
        // visible without a per-session opt-in.
        let config = Config::default();
        assert_eq!(config.nats_leaf_url, "", "precondition: no networking");
        assert!(config.default_share_policy.is_none(), "precondition: not pinned");
        assert_eq!(
            config.effective_default_share_policy(),
            SharePolicyMode::Shared,
            "loopback-only default must be Shared so the local dashboard just works"
        );
    }

    #[test]
    fn effective_share_policy_is_private_when_networked() {
        use open_story_store::event_store::SharePolicyMode;
        // Safe network: the moment a hub/leaf is configured, sharing becomes
        // opt-in so pointing at a network never auto-shares history.
        let config = Config {
            nats_leaf_url: "nats://hub.example:7422".to_string(),
            ..Config::default()
        };
        assert_eq!(
            config.effective_default_share_policy(),
            SharePolicyMode::Private,
            "configuring networking must flip the default to opt-in"
        );
    }

    #[test]
    fn explicit_share_policy_pins_regardless_of_networking() {
        use open_story_store::event_store::SharePolicyMode;
        // An explicit setting wins over the networking-derived default.
        let networked_but_shared = Config {
            nats_leaf_url: "nats://hub.example:7422".to_string(),
            default_share_policy: Some(SharePolicyMode::Shared),
            ..Config::default()
        };
        assert_eq!(
            networked_but_shared.effective_default_share_policy(),
            SharePolicyMode::Shared,
            "explicit Shared must hold even when networked"
        );
        let local_but_private = Config {
            default_share_policy: Some(SharePolicyMode::Private),
            ..Config::default()
        };
        assert_eq!(
            local_but_private.effective_default_share_policy(),
            SharePolicyMode::Private,
            "explicit Private must hold even loopback-only"
        );
    }

    #[test]
    fn env_inputs_is_federated_detects_each_signal() {
        use crate::admin::EnvInputs;
        // No signals → not federated.
        assert!(!EnvInputs::default().is_federated());
        // Each federation signal independently means "networked".
        assert!(EnvInputs {
            hub_domain: Some("hub".into()),
            ..Default::default()
        }
        .is_federated());
        assert!(EnvInputs {
            peer_domains: vec!["peer".into()],
            ..Default::default()
        }
        .is_federated());
        assert!(EnvInputs {
            peer_hub_domains: vec!["ph".into()],
            ..Default::default()
        }
        .is_federated());
        assert!(EnvInputs {
            nats_leafnode_hub: Some("nats://hub".into()),
            ..Default::default()
        }
        .is_federated());
    }

    #[test]
    fn share_default_is_private_when_federated_without_leaf_url() {
        use open_story_store::event_store::SharePolicyMode;
        // The bug: a hub/peer-domain-federated node with an EMPTY nats_leaf_url
        // (federation runs via OPEN_STORY_HUB_DOMAIN, not the leaf URL) used to
        // resolve Shared. Networking is networking — it must default Private.
        let config = Config::default();
        assert_eq!(config.nats_leaf_url, "", "precondition: no leaf URL");
        assert_eq!(
            config.effective_default_share_policy_with(true),
            SharePolicyMode::Private,
            "a domain-federated node must default to opt-in, even with no leaf URL"
        );
        // And the loopback (non-federated) case still defaults Shared.
        assert_eq!(
            config.effective_default_share_policy_with(false),
            SharePolicyMode::Shared,
        );
    }

    #[test]
    fn not_trusted_local_when_federated_without_leaf_url() {
        // The auto-admin bug: a loopback-bound, tokenless, leaf-URL-less node
        // that is federated via hub/peer domains was treated as trusted-local
        // and auto-granted Admin. Federation ⇒ not trusted-local.
        let config = Config::default();
        assert!(
            config.is_trusted_local_with(false),
            "precondition: loopback + no token + not federated IS trusted-local"
        );
        assert!(
            !config.is_trusted_local_with(true),
            "a domain-federated node must NOT auto-admin, even loopback + no token"
        );
    }

    #[test]
    fn apply_answers_preserves_unrelated_fields() {
        let base = Config {
            api_token: "keep-me".into(),
            nats_url: "nats://custom:4222".into(),
            retention_days: 90,
            ..Config::default()
        };
        let answers = WizardAnswers {
            days_history: 7,
            watch_dir: "/w".into(),
            pi_watch_dir: None,
            hermes_watch_dir: None,
            port: 3002,
            data_dir: "./data".into(),
        };
        let config = base.apply_answers(answers);
        assert_eq!(config.api_token, "keep-me", "wizard must not clobber api_token");
        assert_eq!(config.nats_url, "nats://custom:4222");
        assert_eq!(config.retention_days, 90);
    }

    #[test]
    fn write_values_round_trips_chosen_values() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let answers = WizardAnswers {
            days_history: 30,
            watch_dir: "/w".into(),
            pi_watch_dir: Some("/pi".into()),
            hermes_watch_dir: None,
            port: 7777,
            data_dir: tmp.path().to_string_lossy().to_string(),
        };
        let config = Config::default().apply_answers(answers);
        Config::write_values(&path, &config).unwrap();

        let reloaded = Config::from_file(&path);
        assert_eq!(reloaded.port, 7777);
        assert_eq!(reloaded.watch_backfill_hours, 720);
        assert_eq!(reloaded.watch_dir, "/w");
        assert_eq!(reloaded.pi_watch_dir, "/pi");
    }

    #[test]
    fn write_values_is_not_all_comments() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        Config::write_values(&path, &Config::default()).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        // Unlike write_default, this has active (non-comment) settings.
        assert!(
            contents.lines().any(|l| l.starts_with("port =")),
            "write_values should emit an active port line, got:\n{contents}"
        );
    }
}
