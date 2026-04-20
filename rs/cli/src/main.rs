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

use anyhow::Result;
use clap::{Parser, Subcommand};

use open_story::server;
use open_story::server::Config;
use open_story::server::config::{DataBackend, Role};
use open_story::watcher;
use open_story_bus::Bus;
use open_story_bus::nats_bus::NatsBus;
use open_story_store::sqlite_store::SqliteStore;

#[derive(Parser, Debug)]
#[command(name = "open-story", about = "Watch Claude Code transcripts and emit CloudEvents")]
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

        /// Directory to watch for Claude Code transcript files
        #[arg(long, env = "OPEN_STORY_WATCH_DIR")]
        watch_dir: Option<PathBuf>,

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
                    role, host, port, data_dir, static_dir, watch_dir, nats_url,
                    max_initial_records, watch_backfill_hours, truncation_threshold,
                    stale_threshold_secs, api_token, db_key, metrics,
                    data_backend, mongo_uri, mongo_db, init_config,
                }) => ((role, host, port, data_dir, watch_dir, nats_url,
                        max_initial_records, watch_backfill_hours, truncation_threshold,
                        stale_threshold_secs, api_token, db_key, metrics,
                        data_backend, mongo_uri, mongo_db, init_config), static_dir),
                _ => ((Role::Full, None, None, None, None, None, None, None, None, None, None, None, false,
                       None, None, None, false), None),
            };
            let (cli_role, cli_host, cli_port, cli_data_dir, cli_watch_dir, cli_nats_url,
                 cli_max_records, cli_watch_backfill_hours, cli_trunc, cli_stale, cli_api_token,
                 cli_db_key, cli_metrics, cli_data_backend, cli_mongo_uri, cli_mongo_db,
                 init_config) = cli_overrides;

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
            let mut config = Config::from_file(&data_dir.join("config.toml"));
            config.data_dir = data_dir.to_string_lossy().to_string();
            config.role = cli_role;
            if let Some(v) = cli_host { config.host = v; }
            if let Some(v) = cli_port { config.port = v; }
            if let Some(v) = cli_watch_dir { config.watch_dir = v.to_string_lossy().to_string(); }
            if let Some(v) = cli_nats_url { config.nats_url = v; }
            if let Some(v) = cli_max_records { config.max_initial_records = v; }
            if let Some(v) = cli_watch_backfill_hours { config.watch_backfill_hours = v; }
            if let Some(v) = cli_trunc { config.truncation_threshold = v; }
            if let Some(v) = cli_stale { config.stale_threshold_secs = v; }
            if let Some(v) = cli_api_token { config.api_token = v; }
            if let Some(v) = cli_db_key { config.db_key = v; }
            if cli_metrics { config.metrics_enabled = true; }
            if let Some(v) = cli_data_backend { config.data_backend = v; }
            if let Some(v) = cli_mongo_uri { config.mongo_uri = v; }
            if let Some(v) = cli_mongo_db { config.mongo_db = v; }

            // Resolve watch_dir default if not set
            if config.watch_dir.is_empty() {
                config.watch_dir = default_watch_dir().to_string_lossy().to_string();
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
            let watch_dir = PathBuf::from(&config.watch_dir);

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
            let bus: Arc<dyn Bus> = match NatsBus::connect(&nats_url).await {
                Ok(nats_bus) => {
                    if let Err(e) = nats_bus.ensure_streams().await {
                        anyhow::bail!(
                            "NATS stream setup failed: {e}\n\
                             NATS JetStream is required. Install with `brew install nats-server` \
                             and start it (`just up` handles this automatically).\n\
                             NATS URL: {nats_url}"
                        );
                    }
                    eprintln!("  \x1b[2mNATS bus:\x1b[0m        {nats_url}");
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
            };

            server::run_server(&host, port, &data_dir, static_dir.as_deref(), &watch_dir, bus, config).await
        }
        Some(Command::Watch {
            watch_dir,
            output,
            backfill,
            quiet,
        }) => {
            if !watch_dir.exists() {
                anyhow::bail!(
                    "Watch directory does not exist: {}",
                    watch_dir.display()
                );
            }

            let stdout = !quiet;
            let output_file = output.as_deref();

            watcher::watch_directory(&watch_dir, output_file, stdout, backfill)
        }

        Some(Command::Synopsis { session_id, data_dir, format }) => {
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

        Some(Command::Pulse { days, data_dir, format }) => {
            let store = SqliteStore::new(&data_dir)?;
            let pulse = store.with_connection(|conn| {
                open_story_store::queries::project_pulse(conn, days)
            });
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&pulse)?);
            } else {
                if pulse.is_empty() {
                    println!("No activity in the last {days} days.");
                } else {
                    println!("{:<30} {:>8} {:>8}  Last active", "Project", "Sessions", "Events");
                    println!("{}", "-".repeat(70));
                    for p in &pulse {
                        let name = p.project_name.as_deref().unwrap_or(&p.project_id);
                        let last = p.last_activity.as_deref()
                            .and_then(|t| t.get(..10))
                            .unwrap_or("?");
                        println!("{:<30} {:>8} {:>8}  {}", name, p.session_count, p.event_count, last);
                    }
                }
            }
            Ok(())
        }

        Some(Command::Context { project, data_dir, format }) => {
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
                        let last = s.last_event.as_deref()
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
