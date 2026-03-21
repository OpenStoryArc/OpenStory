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
use open_story::server::config::Role;
use open_story::watcher;
use open_story_bus::Bus;
use open_story_bus::nats_bus::NatsBus;
use open_story_bus::noop_bus::NoopBus;
use open_story_semantic::NoopSemanticStore;
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

        /// How far back (hours) to load sessions from JSONL on first boot
        #[arg(long, env = "OPEN_STORY_BOOT_WINDOW_HOURS")]
        boot_window_hours: Option<u64>,

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

        /// Enable semantic search (requires Qdrant)
        #[arg(long, env = "OPEN_STORY_SEMANTIC_ENABLED")]
        semantic_enabled: bool,

        /// Qdrant gRPC endpoint URL
        #[arg(long, env = "OPEN_STORY_QDRANT_URL")]
        qdrant_url: Option<String>,

        /// Path to the ONNX embedding model directory
        #[arg(long, env = "OPEN_STORY_EMBEDDING_MODEL_PATH")]
        embedding_model_path: Option<String>,

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

    /// Backfill semantic embeddings for all existing events
    Backfill {
        /// Directory for persisted session data
        #[arg(long, env = "OPEN_STORY_DATA_DIR", default_value = "./data")]
        data_dir: PathBuf,
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
                    max_initial_records, boot_window_hours, truncation_threshold,
                    stale_threshold_secs, api_token, db_key, metrics,
                    semantic_enabled, qdrant_url, embedding_model_path, init_config,
                }) => ((role, host, port, data_dir, watch_dir, nats_url,
                        max_initial_records, boot_window_hours, truncation_threshold,
                        stale_threshold_secs, api_token, db_key, metrics,
                        semantic_enabled, qdrant_url, embedding_model_path, init_config), static_dir),
                _ => ((Role::Full, None, None, None, None, None, None, None, None, None, None, None, false, false, None, None, false), None),
            };
            let (cli_role, cli_host, cli_port, cli_data_dir, cli_watch_dir, cli_nats_url,
                 cli_max_records, cli_boot_hours, cli_trunc, cli_stale, cli_api_token,
                 cli_db_key, cli_metrics, cli_semantic_enabled, cli_qdrant_url,
                 cli_embedding_model_path, init_config) = cli_overrides;

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
            if let Some(v) = cli_boot_hours { config.boot_window_hours = v; }
            if let Some(v) = cli_trunc { config.truncation_threshold = v; }
            if let Some(v) = cli_stale { config.stale_threshold_secs = v; }
            if let Some(v) = cli_api_token { config.api_token = v; }
            if let Some(v) = cli_db_key { config.db_key = v; }
            if cli_metrics { config.metrics_enabled = true; }
            if cli_semantic_enabled { config.semantic_enabled = true; }
            if let Some(v) = cli_qdrant_url { config.qdrant_url = v; }
            if let Some(v) = cli_embedding_model_path { config.embedding_model_path = v; }

            // Resolve watch_dir default if not set
            if config.watch_dir.is_empty() {
                config.watch_dir = default_watch_dir().to_string_lossy().to_string();
            }

            let host = config.host.clone();
            let port = config.port;
            let nats_url = config.nats_url.clone();
            let watch_dir = PathBuf::from(&config.watch_dir);

            let requires_bus = matches!(config.role, Role::Publisher | Role::Consumer);

            // Connect to NATS event bus (fall back to NoopBus if unavailable)
            let bus: Arc<dyn Bus> = match NatsBus::connect(&nats_url).await {
                Ok(nats_bus) => {
                    if let Err(e) = nats_bus.ensure_streams().await {
                        if requires_bus {
                            anyhow::bail!("NATS stream setup failed: {e} (required for --role {})", config.role);
                        }
                        eprintln!("  \x1b[33mNATS stream setup failed: {e}\x1b[0m");
                        eprintln!("  \x1b[33mFalling back to local mode (no bus)\x1b[0m");
                        Arc::new(NoopBus)
                    } else {
                        eprintln!("  \x1b[2mNATS bus:\x1b[0m        {nats_url}");
                        Arc::new(nats_bus)
                    }
                }
                Err(e) => {
                    if requires_bus {
                        anyhow::bail!("NATS unavailable: {e} (required for --role {})", config.role);
                    }
                    eprintln!("  \x1b[33mNATS unavailable ({e})\x1b[0m");
                    eprintln!("  \x1b[33mRunning in local mode (watcher → direct ingest)\x1b[0m");
                    Arc::new(NoopBus)
                }
            };

            // Semantic search: connect to Qdrant if enabled, else NoopSemanticStore
            let semantic_store: Arc<dyn open_story_semantic::SemanticStore> = if config.semantic_enabled {
                match open_story_semantic::qdrant_store::QdrantStore::new(
                    &config.qdrant_url,
                    open_story_semantic::embedder::EMBEDDING_DIM as u64,
                ).await {
                    Ok(store) => {
                        eprintln!("  \x1b[2mQdrant:\x1b[0m          {}", config.qdrant_url);
                        Arc::new(store)
                    }
                    Err(e) => {
                        eprintln!("  \x1b[33mQdrant unavailable ({e})\x1b[0m");
                        eprintln!("  \x1b[33mSemantic search disabled\x1b[0m");
                        Arc::new(NoopSemanticStore)
                    }
                }
            } else {
                Arc::new(NoopSemanticStore)
            };

            server::run_server(&host, port, &data_dir, static_dir.as_deref(), &watch_dir, bus, semantic_store, config).await
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

        Some(Command::Backfill { data_dir }) => {
            use open_story_semantic::backfill::backfill;
            use open_story_semantic::embedder::{OnnxEmbedder, EMBEDDING_DIM};
            use open_story_store::event_store::EventStore;

            let config = Config::from_file(&data_dir.join("config.toml"));

            let store = SqliteStore::new(&data_dir)?;
            let session_ids: Vec<String> = store
                .list_sessions()?
                .iter()
                .map(|r| r.id.clone())
                .collect();

            if session_ids.is_empty() {
                eprintln!("No sessions found in {}", data_dir.display());
                return Ok(());
            }

            // Load ONNX embedder
            let model_path = if config.embedding_model_path.is_empty() {
                data_dir.join("models")
            } else {
                let p = std::path::PathBuf::from(&config.embedding_model_path);
                if p.is_file() { p.parent().unwrap().to_path_buf() } else { p }
            };
            let embedder = OnnxEmbedder::new(&model_path)?;
            eprintln!("Loaded embedding model from {}", model_path.display());

            // Connect to Qdrant
            let qdrant_url = if config.qdrant_url.is_empty() {
                "http://localhost:6334".to_string()
            } else {
                config.qdrant_url.clone()
            };
            let semantic_store = open_story_semantic::qdrant_store::QdrantStore::new(
                &qdrant_url,
                EMBEDDING_DIM as u64,
            ).await?;
            eprintln!("Connected to Qdrant at {}", qdrant_url);

            eprintln!("Backfilling {} sessions from {}", session_ids.len(), data_dir.display());

            let stats = backfill(
                &session_ids,
                |sid| store.session_events(sid).unwrap_or_default(),
                &embedder,
                &semantic_store,
            )
            .await?;

            eprintln!(
                "\nBackfill complete: {} sessions, {} events scanned, {} chunks embedded, {} skipped, {} errors",
                stats.sessions_processed,
                stats.events_scanned,
                stats.chunks_embedded,
                stats.chunks_skipped,
                stats.errors,
            );
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
