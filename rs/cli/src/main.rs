//! Open Story CLI — thin binary wrapper over the open-story library.
//!
//! Modes:
//!   open-story serve    — HTTP + WebSocket server for the React dashboard (default)
//!   open-story watch    — Watch transcript files and emit CloudEvents to stdout/file
//!   open-story synopsis — Session synopsis query
//!   open-story pulse    — Project activity pulse
//!   open-story context  — Project context for agents

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};

use open_story::server;
use open_story::server::Config;
use open_story::server::config::{DataBackend, Role};
use open_story::watcher;
use open_story_bus::Bus;
use open_story_bus::nats_bus::{Federation, FederationPeers, NatsBus};
use open_story_store::sqlite_store::SqliteStore;

#[derive(Parser, Debug)]
#[command(
    name = "open-story",
    about = "Watch Claude Code transcripts and emit CloudEvents"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the dashboard web server (default)
    Serve {
        /// Server role: full (default), publisher, or consumer
        #[arg(long, env = "OPEN_STORY_ROLE", default_value = "full")]
        role: Role,

        /// Host to bind to
        #[arg(long, env = "OPEN_STORY_HOST")]
        host: Option<String>,

        /// Port to listen on
        #[arg(long, env = "OPEN_STORY_PORT")]
        port: Option<u16>,

        /// Directory for persisted session data (SQLite DB, JSONL, plans)
        #[arg(long, env = "OPEN_STORY_DATA_DIR")]
        data_dir: Option<PathBuf>,

        /// Directory containing built UI static files (index.html, etc.)
        #[arg(long)]
        static_dir: Option<PathBuf>,

        /// Directory to watch for JSONL transcript files. Can be repeated.
        #[arg(long, env = "OPEN_STORY_WATCH_DIR", value_delimiter = ',')]
        watch_dir: Vec<PathBuf>,

        /// Directory to watch for Claude Code transcript files.
        #[arg(long, env = "OPEN_STORY_CLAUDE_WATCH_DIR")]
        claude_watch_dir: Option<PathBuf>,

        /// Directory to watch for Codex rollout JSONL files.
        #[arg(long, env = "OPEN_STORY_CODEX_WATCH_DIR")]
        codex_watch_dir: Option<PathBuf>,

        /// NATS server URL for event bus
        #[arg(long, env = "NATS_URL")]
        nats_url: Option<String>,

        /// Max records in WebSocket initial_state handshake
        #[arg(long, env = "OPEN_STORY_MAX_INITIAL_RECORDS")]
        max_initial_records: Option<usize>,

        /// How far back (hours) the watcher backfills existing JSONL files
        /// in `watch_dir` on startup. Files older than this are skipped.
        /// Set to 0 to disable the filter (useful for tests with static fixtures).
        #[arg(long, env = "OPEN_STORY_WATCH_BACKFILL_HOURS")]
        watch_backfill_hours: Option<u64>,

        /// Payload size (bytes) above which tool outputs are truncated
        #[arg(long, env = "OPEN_STORY_TRUNCATION_THRESHOLD")]
        truncation_threshold: Option<usize>,

        /// Seconds of inactivity before a session shows as stale
        #[arg(long, env = "OPEN_STORY_STALE_THRESHOLD_SECS")]
        stale_threshold_secs: Option<i64>,

        /// Bearer token for API authentication (empty = no auth)
        #[arg(long, env = "OPEN_STORY_API_TOKEN")]
        api_token: Option<String>,

        /// SQLCipher encryption key for the database (empty = unencrypted)
        #[arg(long, env = "OPEN_STORY_DB_KEY")]
        db_key: Option<String>,

        /// Enable Prometheus metrics endpoint at /metrics
        #[arg(long, env = "OPEN_STORY_METRICS")]
        metrics: bool,

        /// Persistence backend: "sqlite" (default) or "mongo".
        /// `mongo` requires building with `--features mongo`.
        #[arg(long, env = "OPEN_STORY_DATA_BACKEND")]
        data_backend: Option<DataBackend>,

        /// MongoDB connection URI. Used only when --data-backend=mongo.
        #[arg(long, env = "OPEN_STORY_MONGO_URI")]
        mongo_uri: Option<String>,

        /// MongoDB database name. Used only when --data-backend=mongo.
        #[arg(long, env = "OPEN_STORY_MONGO_DB")]
        mongo_db: Option<String>,

        /// Write a default config.toml to the data directory and exit
        #[arg(long)]
        init_config: bool,
    },
    /// Watch transcript files and emit CloudEvents
    Watch {
        /// Directory to watch for JSONL transcript files
        #[arg(long, default_value_os_t = default_watch_dir())]
        watch_dir: PathBuf,

        /// Output file for CloudEvents (JSONL append)
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Process existing files before watching
        #[arg(long)]
        backfill: bool,

        /// Suppress stdout output (only write to --output file)
        #[arg(long)]
        quiet: bool,
    },

    /// Show session synopsis — goal, journey, outcome
    Synopsis {
        /// Session ID to query
        session_id: String,

        /// Directory for persisted session data
        #[arg(long, env = "OPEN_STORY_DATA_DIR", default_value = "./data")]
        data_dir: PathBuf,

        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Show project activity pulse — which projects are active
    Pulse {
        /// Number of days to look back
        #[arg(long, default_value = "7")]
        days: u32,

        /// Directory for persisted session data
        #[arg(long, env = "OPEN_STORY_DATA_DIR", default_value = "./data")]
        data_dir: PathBuf,

        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Show project context — recent sessions for a project
    Context {
        /// Project ID to query
        project: String,

        /// Directory for persisted session data
        #[arg(long, env = "OPEN_STORY_DATA_DIR", default_value = "./data")]
        data_dir: PathBuf,

        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Reconcile JSONL on disk → live EventStore (CONSTELLATION R1).
    ///
    /// Walks `data_dir/*.jsonl` and ensures every event is present in the
    /// configured EventStore. Idempotent (PK dedup). No network I/O. Useful
    /// after manually copying JSONL between machines, or after a backend
    /// switch when you don't want to wait for the next server restart.
    /// Boot-time reconciliation runs the same logic automatically.
    /// Write the initial multi-account NATS conf file from `data/config.toml`.
    ///
    /// Solves the chicken-and-egg problem on first boot: nats-server needs
    /// the conf file to exist before it starts, but the server normally
    /// writes the file at runtime via `AccountConfigWriter`. Running this
    /// subcommand first means the operator can start nats-server with the
    /// writer-managed conf BEFORE starting the OpenStory server.
    ///
    /// Reads `nats_accounts_conf_path` + `[person]` from the supplied config,
    /// builds a single PERSON_<NAME> account with one local-dev password
    /// user, and persists to the output path. The subsequent
    /// POST /api/admin/share-with-person calls will mutate the same file
    /// via the writer's atomic rename + SIGHUP path.
    InitAccountsConf {
        /// Path to the OpenStory config file (TOML).
        #[arg(long, default_value = "data/config.toml")]
        config: PathBuf,
        /// Output path for the generated nats-server conf. Defaults to
        /// the `nats_accounts_conf_path` value in the config.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    Reconcile {
        /// Directory for persisted session data (JSONL + EventStore)
        #[arg(long, env = "OPEN_STORY_DATA_DIR", default_value = "./data")]
        data_dir: PathBuf,

        /// Persistence backend: "sqlite" (default) or "mongo".
        /// `mongo` requires building with `--features mongo`.
        #[arg(long, env = "OPEN_STORY_DATA_BACKEND")]
        data_backend: Option<DataBackend>,

        /// MongoDB connection URI. Used only when --data-backend=mongo.
        #[arg(long, env = "OPEN_STORY_MONGO_URI")]
        mongo_uri: Option<String>,

        /// MongoDB database name. Used only when --data-backend=mongo.
        #[arg(long, env = "OPEN_STORY_MONGO_DB")]
        mongo_db: Option<String>,

        /// SQLCipher encryption key for the database (empty = unencrypted)
        #[arg(long, env = "OPEN_STORY_DB_KEY")]
        db_key: Option<String>,

        /// Print per-session error detail (otherwise only first 5 errors shown)
        #[arg(long)]
        verbose: bool,
    },
}

fn default_watch_dir() -> PathBuf {
    dirs_path().unwrap_or_else(|| PathBuf::from("."))
}

fn dirs_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(|p| PathBuf::from(p).join(".claude").join("projects"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .ok()
            .map(|p| PathBuf::from(p).join(".claude").join("projects"))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Serve { .. }) => {
            let (cli_overrides, static_dir) = match cli.command {
                Some(Command::Serve {
                    role,
                    host,
                    port,
                    data_dir,
                    static_dir,
                    watch_dir,
                    claude_watch_dir,
                    codex_watch_dir,
                    nats_url,
                    max_initial_records,
                    watch_backfill_hours,
                    truncation_threshold,
                    stale_threshold_secs,
                    api_token,
                    db_key,
                    metrics,
                    data_backend,
                    mongo_uri,
                    mongo_db,
                    init_config,
                }) => (
                    (
                        role,
                        host,
                        port,
                        data_dir,
                        watch_dir,
                        claude_watch_dir,
                        codex_watch_dir,
                        nats_url,
                        max_initial_records,
                        watch_backfill_hours,
                        truncation_threshold,
                        stale_threshold_secs,
                        api_token,
                        db_key,
                        metrics,
                        data_backend,
                        mongo_uri,
                        mongo_db,
                        init_config,
                    ),
                    static_dir,
                ),
                _ => (
                    (
                        Role::Full,
                        None,
                        None,
                        None,
                        Vec::new(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                        false,
                    ),
                    None,
                ),
            };
            let (
                cli_role,
                cli_host,
                cli_port,
                cli_data_dir,
                cli_watch_dir,
                cli_claude_watch_dir,
                cli_codex_watch_dir,
                cli_nats_url,
                cli_max_records,
                cli_watch_backfill_hours,
                cli_trunc,
                cli_stale,
                cli_api_token,
                cli_db_key,
                cli_metrics,
                cli_data_backend,
                cli_mongo_uri,
                cli_mongo_db,
                init_config,
            ) = cli_overrides;

            // Resolve data_dir first (needed to find config.toml)
            let data_dir = cli_data_dir.unwrap_or_else(|| PathBuf::from("./data"));

            // Handle --init-config
            if init_config {
                std::fs::create_dir_all(&data_dir)?;
                let config_path = data_dir.join("config.toml");
                Config::write_default(&config_path)?;
                eprintln!("Wrote default config to {}", config_path.display());
                return Ok(());
            }

            // Load config: defaults → config.toml → CLI flags
            let config_path = data_dir.join("config.toml");
            let mut config = Config::from_file(&config_path);
            // First-boot: auto-create [person] section with detected (host, user)
            // matchers and persist back to config.toml. Idempotent on later boots.
            std::fs::create_dir_all(&data_dir).ok();
            config.ensure_person_bootstrap(&config_path);
            config.data_dir = data_dir.to_string_lossy().to_string();
            config.role = cli_role;
            if let Some(v) = cli_host {
                config.host = v;
            }
            if let Some(v) = cli_port {
                config.port = v;
            }
            if !cli_watch_dir.is_empty() {
                config.watch_dirs = cli_watch_dir
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect();
                if let Some(first) = config.watch_dirs.first() {
                    config.watch_dir = first.clone();
                }
            }
            if let Some(v) = cli_claude_watch_dir {
                config.claude_watch_dir = v.to_string_lossy().to_string();
            }
            if let Some(v) = cli_codex_watch_dir {
                config.codex_watch_dir = v.to_string_lossy().to_string();
            }
            if let Some(v) = cli_nats_url {
                config.nats_url = v;
            }
            if let Some(v) = cli_max_records {
                config.max_initial_records = v;
            }
            if let Some(v) = cli_watch_backfill_hours {
                config.watch_backfill_hours = v;
            }
            if let Some(v) = cli_trunc {
                config.truncation_threshold = v;
            }
            if let Some(v) = cli_stale {
                config.stale_threshold_secs = v;
            }
            if let Some(v) = cli_api_token {
                config.api_token = v;
            }
            if let Some(v) = cli_db_key {
                config.db_key = v;
            }
            if cli_metrics {
                config.metrics_enabled = true;
            }
            if let Some(v) = cli_data_backend {
                config.data_backend = v;
            }
            if let Some(v) = cli_mongo_uri {
                config.mongo_uri = v;
            }
            if let Some(v) = cli_mongo_db {
                config.mongo_db = v;
            }

            // Resolve watch_dir default if not set
            if config.watch_dir.is_empty() {
                config.watch_dir = default_watch_dir().to_string_lossy().to_string();
            }
            if config.watch_dirs.is_empty() {
                for candidate in [
                    config.claude_watch_dir.clone(),
                    config.codex_watch_dir.clone(),
                    config.watch_dir.clone(),
                ] {
                    if !candidate.is_empty() && !config.watch_dirs.contains(&candidate) {
                        config.watch_dirs.push(candidate);
                    }
                }
            }

            // Pi-mono watch dir from env var (config.toml also works)
            if config.pi_watch_dir.is_empty() {
                if let Ok(v) = std::env::var("OPEN_STORY_PI_WATCH_DIR") {
                    config.pi_watch_dir = v;
                }
            }

            // Hermes watch dir from env var (config.toml also works)
            if config.hermes_watch_dir.is_empty() {
                if let Ok(v) = std::env::var("OPEN_STORY_HERMES_WATCH_DIR") {
                    config.hermes_watch_dir = v;
                }
            }

            let host = config.host.clone();
            let port = config.port;
            let nats_url = config.nats_url.clone();
            let watch_dirs: Vec<PathBuf> = config.watch_dirs.iter().map(PathBuf::from).collect();

            // NATS JetStream is a hard requirement. The reactive actor
            // decomposition (persist / patterns / projections / broadcast)
            // subscribes to events.> and owns one responsibility each —
            // without a real bus the actors are dormant and the pipeline
            // collapses. Failing fast here keeps the system honest: it
            // either runs as designed or tells you why it can't.
            //
            // To enable a no-NATS demo path in the future, build a
            // first-class InProcessBus that actually delivers to the
            // consumers — don't resurrect NoopBus here.
            // Federation mode is opted in via env vars:
            //   OPEN_STORY_HUB_DOMAIN=<dom>       T2/T3 hub-star (with hubs)
            //   OPEN_STORY_PEER_DOMAINS=<comma>   T1 solo multi-device mesh
            // Hub role + hub_domain → hub-side: create events-agg.
            // Leaf + hub_domain → Hub federation peers (Phase 2a/b).
            // Leaf + peer_domains → Mesh federation peers (Phase 2b Step 4).
            // Both unset → solo, as before.
            let hub_domain = std::env::var("OPEN_STORY_HUB_DOMAIN").ok().filter(|s| !s.is_empty());
            let peer_domains_raw = std::env::var("OPEN_STORY_PEER_DOMAINS").ok().filter(|s| !s.is_empty());
            let peer_domains: Option<Vec<String>> = peer_domains_raw.as_ref().map(|s| {
                s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
            });
            let is_hub = matches!(config.role, Role::Consumer) && hub_domain.is_some();

            // T3 multi-hub mesh: a hub also sources peer hubs' aggregates.
            let peer_hub_domains: Vec<String> = std::env::var("OPEN_STORY_PEER_HUB_DOMAINS")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
                .unwrap_or_default();

            let bus: Arc<dyn Bus> = if let Some(dom) = hub_domain.clone().filter(|_| is_hub) {
                // Hub role: own NATS configured with `domain: <dom>`; create
                // events-agg so leaves can self-register sources into it.
                // In T3, also source peer hubs' events-agg streams.
                match NatsBus::connect_hub(&nats_url, &dom).await {
                    Ok(nats_bus) => {
                        nats_bus.ensure_streams().await
                            .with_context(|| "NATS stream setup (hub) failed")?;
                        nats_bus.ensure_aggregate(&peer_hub_domains).await
                            .with_context(|| "NATS events-agg setup (hub) failed")?;
                        let peer_label = if peer_hub_domains.is_empty() { String::new() } else { format!(" peers=[{}]", peer_hub_domains.join(",")) };
                        eprintln!("  \x1b[2mNATS bus:\x1b[0m        {nats_url} (federation: hub domain={dom}{peer_label})");
                        Arc::new(nats_bus)
                    }
                    Err(e) => anyhow::bail!("NATS unavailable (hub): {e}\nNATS URL: {nats_url}"),
                }
            } else if let Some(dom) = hub_domain {
                // Leaf attached to a hub (T2/T3).
                let host = open_story_core::host::host().to_string();
                let fed = Federation {
                    host: host.clone(),
                    peers: FederationPeers::Hub { hub_domain: dom.clone() },
                };
                match NatsBus::connect_federation(&nats_url, fed).await {
                    Ok(nats_bus) => {
                        nats_bus.ensure_streams().await
                            .with_context(|| format!("NATS stream setup (leaf, hub={dom}) failed"))?;
                        eprintln!(
                            "  \x1b[2mNATS bus:\x1b[0m        {nats_url} (federation: leaf host={host} → hub={dom})"
                        );
                        Arc::new(nats_bus)
                    }
                    Err(e) => anyhow::bail!("NATS unavailable (leaf): {e}\nNATS URL: {nats_url}"),
                }
            } else if let Some(peers) = peer_domains {
                // T1 solo multi-device mesh: no hub, source from each peer.
                let host = open_story_core::host::host().to_string();
                // Filter the host out of its own peer list defensively — the
                // operator might pass the full fleet list to every device.
                let peer_filtered: Vec<String> = peers.into_iter().filter(|p| p != &host).collect();
                let fed = Federation {
                    host: host.clone(),
                    peers: FederationPeers::Mesh { peer_domains: peer_filtered.clone() },
                };
                match NatsBus::connect_federation(&nats_url, fed).await {
                    Ok(nats_bus) => {
                        nats_bus.ensure_streams().await
                            .with_context(|| format!("NATS stream setup (mesh, peers={peer_filtered:?}) failed"))?;
                        eprintln!(
                            "  \x1b[2mNATS bus:\x1b[0m        {nats_url} (federation: mesh host={host} peers={})",
                            peer_filtered.join(",")
                        );
                        Arc::new(nats_bus)
                    }
                    Err(e) => anyhow::bail!("NATS unavailable (mesh leaf): {e}\nNATS URL: {nats_url}"),
                }
            } else {
                // Solo.
                match NatsBus::connect(&nats_url).await {
                    Ok(nats_bus) => {
                        if let Err(e) = nats_bus.ensure_streams().await {
                            anyhow::bail!(
                                "NATS stream setup failed: {e}\n\
                                 NATS JetStream is required. Install with `brew install nats-server` \
                                 and start it (`just up` handles this automatically).\n\
                                 NATS URL: {nats_url}"
                            );
                        }
                        eprintln!("  \x1b[2mNATS bus:\x1b[0m        {nats_url} (solo)");
                        Arc::new(nats_bus)
                    }
                    Err(e) => {
                        anyhow::bail!(
                            "NATS unavailable: {e}\n\
                             NATS JetStream is required. Install with `brew install nats-server` \
                             and start it (`just up` handles this automatically).\n\
                             NATS URL: {nats_url}"
                        );
                    }
                }
            };

            server::run_server(
                &host,
                port,
                &data_dir,
                static_dir.as_deref(),
                &watch_dirs,
                bus,
                config,
            )
            .await
        }
        Some(Command::Watch {
            watch_dir,
            output,
            backfill,
            quiet,
        }) => {
            if !watch_dir.exists() {
                anyhow::bail!("Watch directory does not exist: {}", watch_dir.display());
            }

            let stdout = !quiet;
            let output_file = output.as_deref();

            watcher::watch_directory(&watch_dir, output_file, stdout, backfill)
        }

        Some(Command::Synopsis {
            session_id,
            data_dir,
            format,
        }) => {
            let store = SqliteStore::new(&data_dir)?;
            let synopsis = store.with_connection(|conn| {
                open_story_store::queries::session_synopsis(conn, &session_id)
            });
            match synopsis {
                Some(s) => {
                    if format == "json" {
                        println!("{}", serde_json::to_string_pretty(&s)?);
                    } else {
                        println!("Session: {}", s.session_id);
                        if let Some(label) = &s.label {
                            println!("Label:   {label}");
                        }
                        if let Some(project) = &s.project_name {
                            println!("Project: {project}");
                        }
                        println!("Events:  {}", s.event_count);
                        println!("Tools:   {}", s.tool_count);
                        println!("Errors:  {}", s.error_count);
                        if let Some(d) = s.duration_secs {
                            let mins = d / 60;
                            let secs = d % 60;
                            println!("Duration: {mins}m {secs}s");
                        }
                        if !s.top_tools.is_empty() {
                            println!("\nTop tools:");
                            for t in &s.top_tools {
                                println!("  {:<12} {}", t.tool, t.count);
                            }
                        }
                    }
                    Ok(())
                }
                None => {
                    eprintln!("Session not found: {session_id}");
                    std::process::exit(1);
                }
            }
        }

        Some(Command::Pulse {
            days,
            data_dir,
            format,
        }) => {
            let store = SqliteStore::new(&data_dir)?;
            let pulse =
                store.with_connection(|conn| open_story_store::queries::project_pulse(conn, days));
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&pulse)?);
            } else {
                if pulse.is_empty() {
                    println!("No activity in the last {days} days.");
                } else {
                    println!(
                        "{:<30} {:>8} {:>8}  Last active",
                        "Project", "Sessions", "Events"
                    );
                    println!("{}", "-".repeat(70));
                    for p in &pulse {
                        let name = p.project_name.as_deref().unwrap_or(&p.project_id);
                        let last = p
                            .last_activity
                            .as_deref()
                            .and_then(|t| t.get(..10))
                            .unwrap_or("?");
                        println!(
                            "{:<30} {:>8} {:>8}  {}",
                            name, p.session_count, p.event_count, last
                        );
                    }
                }
            }
            Ok(())
        }

        Some(Command::Reconcile {
            data_dir,
            data_backend,
            mongo_uri,
            mongo_db,
            db_key,
            verbose,
        }) => {
            use open_story_store::state::{BackendChoice, StoreState};

            // Load config: defaults → config.toml → CLI flags / env (mirrors `serve`).
            let config_path = data_dir.join("config.toml");
            let mut config = Config::from_file(&config_path);
            std::fs::create_dir_all(&data_dir).ok();
            config.ensure_person_bootstrap(&config_path);
            config.data_dir = data_dir.to_string_lossy().to_string();
            if let Some(v) = data_backend {
                config.data_backend = v;
            }
            if let Some(v) = mongo_uri {
                config.mongo_uri = v;
            }
            if let Some(v) = mongo_db {
                config.mongo_db = v;
            }
            if let Some(v) = db_key {
                config.db_key = v;
            }

            let backend = match config.data_backend {
                DataBackend::Sqlite => BackendChoice::Sqlite,
                DataBackend::Mongo => BackendChoice::Mongo {
                    uri: config.mongo_uri.clone(),
                    db_name: config.mongo_db.clone(),
                },
            };
            let key = if config.db_key.is_empty() {
                None
            } else {
                Some(config.db_key.as_str())
            };

            let mut store = StoreState::with_backend(&data_dir, key, backend).await?;
            let report =
                open_story::server::reconcile::reconcile_local(&data_dir, &mut store).await?;

            println!(
                "Reconciled {} JSONL files: {} events added, {} skipped, {} sessions upserted in {:.2}s",
                report.files_walked,
                report.events_inserted,
                report.events_skipped,
                report.sessions_upserted,
                report.elapsed.as_secs_f64(),
            );
            if !report.errors.is_empty() {
                let cap = if verbose { report.errors.len() } else { 5 };
                eprintln!("\n{} error(s):", report.errors.len());
                for err in report.errors.iter().take(cap) {
                    eprintln!("  - {err}");
                }
                if !verbose && report.errors.len() > cap {
                    eprintln!(
                        "  ... and {} more (rerun with --verbose for full list)",
                        report.errors.len() - cap
                    );
                }
            }
            Ok(())
        }

        Some(Command::InitAccountsConf { config, output }) => {
            use open_story::server::account_config::{
                AccountConfigWriter, DEFAULT_NATS_STATIC_PREFIX,
            };
            use open_story_bus::accounts::{AccountSpec, UserSpec};

            let raw = std::fs::read_to_string(&config)
                .map_err(|e| anyhow::anyhow!("read {}: {e}", config.display()))?;
            let cfg: open_story::server::config::Config = toml::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("parse {}: {e}", config.display()))?;

            let output_path = output
                .or_else(|| {
                    if cfg.nats_accounts_conf_path.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(&cfg.nats_accounts_conf_path))
                    }
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "no output: pass --output or set `nats_accounts_conf_path` in {}",
                        config.display()
                    )
                })?;

            let person = cfg.person.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "no [person] section in {} — cannot build accounts conf",
                    config.display()
                )
            })?;

            let account_name = format!(
                "PERSON_{}",
                person.id.to_uppercase().replace('-', "_")
            );
            let local_account = AccountSpec {
                name: account_name.clone(),
                users: vec![UserSpec {
                    user: person.id.clone(),
                    // Deterministic local-dev password — matches the boot path
                    // (`build_account_config` in state.rs). NOT a security
                    // control by itself; swap to NKEY for any deployment that
                    // crosses an untrusted network.
                    password: format!("{}-local-dev", person.id),
                    permissions: None,
                }],
                exports: vec![],
                imports: vec![],
            };

            let writer = AccountConfigWriter::new(
                output_path.clone(),
                DEFAULT_NATS_STATIC_PREFIX,
                vec![local_account],
            );
            writer
                .persist()
                .map_err(|e| anyhow::anyhow!("persist to {}: {e}", output_path.display()))?;

            println!(
                "✓ wrote initial accounts conf to {}",
                output_path.display()
            );
            println!("  account: {account_name}");
            println!("  user:    {} (password: {}-local-dev)", person.id, person.id);
            println!();
            println!("Next:");
            println!(
                "  nats-server -c {} &disown",
                output_path.display()
            );
            println!("  just serve");
            Ok(())
        }

        Some(Command::Context {
            project,
            data_dir,
            format,
        }) => {
            let store = SqliteStore::new(&data_dir)?;
            let context = store.with_connection(|conn| {
                open_story_store::queries::project_context(conn, &project, 5)
            });
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&context)?);
            } else {
                if context.is_empty() {
                    println!("No sessions found for project: {project}");
                } else {
                    println!("Recent sessions for \"{project}\":\n");
                    for s in &context {
                        let label = s.label.as_deref().unwrap_or("(no label)");
                        let last = s
                            .last_event
                            .as_deref()
                            .and_then(|t| t.get(..19))
                            .unwrap_or("?");
                        println!("  {} | {} events | {}", last, s.event_count, label);
                    }
                }
            }
            Ok(())
        }
    }
}
