use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use arena::cli::{
    cmd_down, cmd_keygen, cmd_serve, cmd_up, cmd_users, docker_driver_config_from_env,
    litellm_minter_from_env,
};
use arena::db::Db;
use arena::docker_driver::DockerDriver;
use arena::state::ArenaConfig;

/// Open Story Arena — ephemeral, sealed coding-agent sandboxes for events.
///
/// `serve`, `down`, and (indirectly, via `Db::open`) `up`/`users` are all
/// configured from the same `ARENA_*` env vars as the HTTP server; see
/// `state::ArenaConfig::from_env`.
#[derive(Parser)]
#[command(name = "arena", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the arena HTTP server. Env-configured: ARENA_BASE_DOMAIN,
    /// ARENA_COOKIE_KEY, ARENA_DB, ARENA_DOCKER_RUNTIME, ARENA_LITELLM_URL,
    /// ARENA_LISTEN, ARENA_EDGE_CONTAINER, ARENA_LITELLM_CONTAINER,
    /// ARENA_SANDBOX_CPUS, ARENA_SANDBOX_MEMORY_BYTES, LITELLM_MASTER_KEY.
    Serve,

    /// Stand up an event from a manifest TOML file. Prints roster
    /// credentials as CSV, or a join-code confirmation line.
    Up {
        /// Path to the event manifest (e.g. events/example-event.toml).
        manifest: PathBuf,
    },

    /// Tear down every sandbox belonging to an event: destroys each
    /// container, revokes its LiteLLM key, and removes its database row.
    Down {
        /// Event name, as declared in its manifest's `name` field.
        event: String,
    },

    /// List the usernames registered for an event, one per line.
    Users {
        /// Event name, as declared in its manifest's `name` field.
        event: String,
    },

    /// Generate a random 64-byte hex key, suitable for ARENA_COOKIE_KEY.
    Keygen,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve => {
            let cfg = ArenaConfig::from_env()?;
            cmd_serve(cfg).await?;
        }
        Command::Up { manifest } => {
            let cfg = ArenaConfig::from_env()?;
            let db = Db::open(&cfg.db_path)?;
            let out = cmd_up(&db, &manifest)?;
            print!("{out}");
        }
        Command::Down { event } => {
            let cfg = ArenaConfig::from_env()?;
            let db = Db::open(&cfg.db_path)?;
            // Same real Docker driver + LiteLLM minter as `serve`, wired
            // from the same env vars — see cli::docker_driver_config_from_env
            // and cli::litellm_minter_from_env.
            let driver = DockerDriver::connect(docker_driver_config_from_env(&cfg))?;
            let minter = litellm_minter_from_env(&cfg)?;
            let count = cmd_down(&db, &driver, &minter, &event).await?;
            println!("destroyed {count} sandbox(es) for event {event:?}");
        }
        Command::Users { event } => {
            let cfg = ArenaConfig::from_env()?;
            let db = Db::open(&cfg.db_path)?;
            let out = cmd_users(&db, &event)?;
            println!("{out}");
        }
        Command::Keygen => {
            println!("{}", cmd_keygen());
        }
    }

    Ok(())
}
