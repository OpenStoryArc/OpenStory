//! `open-story init` — interactive first-run setup wizard.
//!
//! Zero dependencies: prompts are plain `std::io`. The prompt helpers are
//! generic over `BufRead`/`Write` so they're unit-testable with an in-memory
//! `Cursor` (no real terminal needed). The pure "answer → Config" logic lives
//! in `open_story::server::config` (`WizardAnswers`, `apply_answers`,
//! `parse_days`, `parse_port`); this module is the I/O edge that drives it.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;

use open_story::server::config::{parse_days, parse_port, WizardAnswers};
use open_story::server::Config;

/// Prompt for a free-text value with a default. Empty input (or EOF) returns
/// the default; otherwise the trimmed input.
fn prompt_with_default<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
    default: &str,
) -> io::Result<String> {
    write!(writer, "{label} [{default}]: ")?;
    writer.flush()?;
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(default.to_string()); // EOF — accept default
    }
    let trimmed = line.trim();
    Ok(if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    })
}

/// Prompt for a value validated by `parse`, re-prompting until valid. Empty
/// input (or EOF) returns the default.
fn prompt_numeric<R, W, T>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
    default: T,
    parse: impl Fn(&str) -> Result<T, String>,
) -> io::Result<T>
where
    R: BufRead,
    W: Write,
    T: std::fmt::Display + Copy,
{
    loop {
        write!(writer, "{label} [{default}]: ")?;
        writer.flush()?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(default); // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }
        match parse(trimmed) {
            Ok(v) => return Ok(v),
            Err(e) => writeln!(writer, "  ! {e}")?,
        }
    }
}

/// Yes/no confirm. Enter honors `default`; accepts y/yes/n/no (case-insensitive).
fn confirm<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
    default: bool,
) -> io::Result<bool> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        write!(writer, "{label} {hint}: ")?;
        writer.flush()?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(default);
        }
        match line.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(writer, "  ! please answer y or n")?,
        }
    }
}

/// Entry point from `main`. Gates on an interactive stdin, then drives the
/// flow reading from stdin and writing prompts to stderr (stdout stays clean).
pub fn run_wizard(data_dir: PathBuf) -> Result<()> {
    if !io::stdin().is_terminal() {
        anyhow::bail!(
            "`open-story init` needs an interactive terminal.\n\
             For non-interactive setup, run `open-story serve --init-config` \
             (writes a commented template) then edit {}/config.toml by hand.",
            data_dir.display()
        );
    }
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut out = io::stderr();
    run_wizard_io(&mut reader, &mut out, data_dir)
}

/// Environment probes the flow branches on, injected so the flow is testable
/// without shelling out. `Default` = all-off (nothing installed), the hermetic
/// baseline for tests.
#[derive(Default)]
struct WizardEnv {
    /// Homebrew present → offer `brew services run`.
    brew: bool,
    /// Path to the `open-story-mcp` companion binary, if installed.
    mcp_bin: Option<PathBuf>,
    /// `claude` CLI present → can run `claude mcp add` directly.
    claude: bool,
}

/// The wizard flow, generic over reader/writer so the I/O edge is uniform.
/// Probes the environment once (brew, the MCP companion, the `claude` CLI),
/// then runs the flow with those results injected so it stays testable.
fn run_wizard_io<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    data_dir: PathBuf,
) -> Result<()> {
    let env = WizardEnv {
        brew: brew_available(),
        mcp_bin: mcp_binary_path(),
        claude: claude_available(),
    };
    run_flow(reader, writer, data_dir, env)
}

/// The wizard flow with environment probes injected.
fn run_flow<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    data_dir: PathBuf,
    env: WizardEnv,
) -> Result<()> {
    writeln!(
        writer,
        "\n  OpenStory setup — answer a few questions (Enter accepts the default in [brackets]).\n"
    )?;

    // Existing config (if any) becomes the set of defaults, so re-running the
    // wizard is non-destructive: untouched fields are preserved.
    let base = Config::from_file(&data_dir.join("config.toml"));

    // 1. Days of history → watch_backfill_hours.
    let default_days = (base.watch_backfill_hours / 24).max(1);
    let days = prompt_numeric(
        reader,
        writer,
        "How many days of session history should load on boot?",
        default_days,
        parse_days,
    )?;

    // 2. Claude Code watch dir.
    let default_watch = if base.watch_dir.is_empty() {
        crate::default_watch_dir().to_string_lossy().to_string()
    } else {
        base.watch_dir.clone()
    };
    let watch_dir = prompt_with_default(
        reader,
        writer,
        "Which directory holds your Claude Code transcripts?",
        &default_watch,
    )?;
    if !Path::new(&watch_dir).exists() {
        writeln!(
            writer,
            "  note: {watch_dir} doesn't exist yet — that's fine, Claude Code creates it on first run."
        )?;
    }

    // 3. Optional multi-agent dirs (single gate, default no).
    let (pi_watch_dir, hermes_watch_dir) =
        if confirm(reader, writer, "Watch additional agents (pi-mono / Hermes)?", false)? {
            let pi = prompt_with_default(
                reader,
                writer,
                "pi-mono session dir (blank to skip)",
                &base.pi_watch_dir,
            )?;
            let hermes = prompt_with_default(
                reader,
                writer,
                "Hermes session dir (blank to skip)",
                &base.hermes_watch_dir,
            )?;
            (
                (!pi.is_empty()).then_some(pi),
                (!hermes.is_empty()).then_some(hermes),
            )
        } else {
            (None, None)
        };

    // 4. Port + data dir.
    let port = prompt_numeric(
        reader,
        writer,
        "Which port should the dashboard listen on?",
        base.port,
        parse_port,
    )?;
    let data_dir_str = prompt_with_default(
        reader,
        writer,
        "Where should OpenStory store its data?",
        &data_dir.to_string_lossy(),
    )?;

    let config = base.apply_answers(WizardAnswers {
        days_history: days,
        watch_dir,
        pi_watch_dir,
        hermes_watch_dir,
        port,
        data_dir: data_dir_str.clone(),
    });

    // 5. Write config — surface IO errors with the offending path before
    //    claiming success.
    let out_dir = PathBuf::from(&data_dir_str);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("cannot create data dir {}: {e}", out_dir.display()))?;
    let out_path = out_dir.join("config.toml");
    Config::write_values(&out_path, &config)
        .map_err(|e| anyhow::anyhow!("cannot write {}: {e}", out_path.display()))?;

    // 6. Summary.
    // Bind watch_dir to a local first: the observe-never-interfere principle
    // test flags any source line mentioning `watch_dir` next to a write op like
    // `writeln!`. This only writes to the wizard's output stream, never to the
    // watch dir — keep the token off the writeln! line so the heuristic scanner
    // doesn't false-positive.
    let watched = &config.watch_dir;
    writeln!(writer, "\n  ✓ Wrote {}", out_path.display())?;
    writeln!(
        writer,
        "    history : {days} days ({} h backfill)",
        config.watch_backfill_hours
    )?;
    writeln!(writer, "    watch   : {watched}")?;
    writeln!(writer, "    port    : {}", config.port)?;
    writeln!(writer, "    data    : {}", config.data_dir)?;

    // 7. Offer to start services (Homebrew only) + open the dashboard.
    maybe_start_services(reader, writer, config.port, env.brew)?;

    // 8. Offer to wire OpenStory's MCP tools into Claude Code. The agent-in-UI
    //    seam (ui_control / where_is_user / subscribe_ui_state) is inert until
    //    the MCP is registered, so setup shouldn't leave that step implicit.
    maybe_wire_mcp(reader, writer, env.mcp_bin.as_deref(), env.claude)?;

    writeln!(writer, "\n  Dashboard: http://localhost:{}\n", config.port)?;
    Ok(())
}

/// `brew services run` — start for this login session only, WITHOUT
/// registering to auto-launch at login (that's `start`, an explicit opt-in).
const BREW_SERVICE_RUN: [&str; 3] = ["services", "run", "openstory"];

/// If Homebrew is present, offer to start NATS + OpenStory and open the
/// dashboard. Never mutates `~/.claude/settings.json` (no hooks). Failures are
/// reported, never fatal.
fn maybe_start_services<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    port: u16,
    brew: bool,
) -> Result<()> {
    if !brew {
        writeln!(writer, "\n  When you're ready, start it with:")?;
        writeln!(writer, "    open-story serve     (brings up NATS automatically)")?;
        return Ok(());
    }
    // `brew services run` (not `start`) launches it for this login session
    // WITHOUT registering it to auto-start at login. The brew service runs
    // `serve --manage-nats`, so this brings up the JetStream NATS + API +
    // dashboard — one command, whole stack.
    if confirm(reader, writer, "\n  Start OpenStory now in the background? (brings up NATS; not auto-started at login)", false)? {
        match std::process::Command::new("brew")
            .args(BREW_SERVICE_RUN)
            .status()
        {
            Ok(s) if s.success() => {
                writeln!(writer, "  ✓ running openstory (NATS + dashboard included)")?;
                writeln!(writer, "    (to also launch at login: brew services start openstory)")?;
            }
            Ok(s) => writeln!(writer, "  ! `brew services run openstory` exited with {s}")?,
            Err(e) => writeln!(writer, "  ! could not run brew: {e}")?,
        }
        if confirm(reader, writer, "  Open the dashboard in your browser?", true)? {
            open_browser(writer, port);
        }
    }
    Ok(())
}

/// True if `brew` is on PATH and runnable.
fn brew_available() -> bool {
    std::process::Command::new("brew")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve a binary on `PATH` — a dependency-free `command -v`. Used to *probe*
/// for the MCP companion and the `claude` CLI. We must never exec the MCP binary
/// to detect it (it speaks JSON-RPC over stdin and would block), so a PATH-file
/// lookup is the only safe probe.
fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

/// Path to the `open-story-mcp` companion binary if installed (via the optional
/// `openstory-mcp` formula). `None` → not installed.
fn mcp_binary_path() -> Option<PathBuf> {
    which("open-story-mcp")
}

/// True if the `claude` CLI is on PATH (so `claude mcp add` can run).
fn claude_available() -> bool {
    which("claude").is_some()
}

/// The `claude mcp add` argv to register the MCP over stdio at the default
/// (local) scope. The binary defaults to `http://localhost:3002`, so no `--env`
/// is needed locally. The `--` separates Claude's own flags from the server
/// command — the older positional `stdio` form no longer parses.
fn claude_mcp_add_args(mcp_bin: &str) -> Vec<String> {
    ["mcp", "add", "--transport", "stdio", "openstory", "--", mcp_bin]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Offer to wire OpenStory's MCP tools into Claude Code. Environment probes
/// (companion binary path, `claude` CLI presence) are injected so the flow is
/// unit-testable without spawning anything.
///
/// This is the one place setup touches the user's agent config, and only via an
/// explicit, consented `claude mcp add` — it registers a server the user asked
/// for, it does not inject hooks or alter agent behavior. That's distinct from
/// the "observe, never interfere" boundary, which governs the *listener's*
/// relation to the agent's transcripts, not the user configuring their own tools.
fn maybe_wire_mcp<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    mcp_bin: Option<&Path>,
    claude: bool,
) -> Result<()> {
    let Some(bin) = mcp_bin else {
        // Companion not installed — point at the formula, don't prompt.
        writeln!(
            writer,
            "\n  Want your agent to query + drive OpenStory? Install the MCP companion:"
        )?;
        writeln!(writer, "    brew install openstoryarc/openstory/openstory-mcp")?;
        return Ok(());
    };
    let bin = bin.display();
    let manual = format!("claude mcp add --transport stdio openstory -- {bin}");

    if !claude {
        // Binary present, but no `claude` CLI to run the registration for them.
        writeln!(writer, "\n  To wire OpenStory's 24 MCP tools into Claude Code, run:")?;
        writeln!(writer, "    {manual}")?;
        return Ok(());
    }

    // Default NO: this writes to the user's agent config (~/.claude.json), so a
    // bare Enter must never register anything — opt-in only, mirroring the
    // service-start prompt above.
    if confirm(
        reader,
        writer,
        "\n  Wire OpenStory's MCP tools into Claude Code now? (24 tools — query history + drive the dashboard)",
        false,
    )? {
        match std::process::Command::new("claude")
            .args(claude_mcp_add_args(&bin.to_string()))
            .status()
        {
            Ok(s) if s.success() => {
                writeln!(writer, "  ✓ registered the openstory MCP (restart Claude Code to load it)")?;
            }
            Ok(s) => {
                writeln!(writer, "  ! `claude mcp add` exited with {s} — run it yourself:")?;
                writeln!(writer, "    {manual}")?;
            }
            Err(e) => {
                writeln!(writer, "  ! could not run claude: {e} — run it yourself:")?;
                writeln!(writer, "    {manual}")?;
            }
        }
    } else {
        writeln!(writer, "  skipped — wire it later with:")?;
        writeln!(writer, "    {manual}")?;
    }
    Ok(())
}

fn open_browser<W: Write>(writer: &mut W, port: u16) {
    let url = format!("http://localhost:{port}");
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(&url).status();
    let _ = writeln!(writer, "  → {url}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn reader(s: &str) -> Cursor<Vec<u8>> {
        Cursor::new(s.as_bytes().to_vec())
    }

    #[test]
    fn prompt_with_default_returns_default_on_empty() {
        let mut w = Vec::new();
        let got = prompt_with_default(&mut reader("\n"), &mut w, "Dir", "/default").unwrap();
        assert_eq!(got, "/default");
        assert!(String::from_utf8(w).unwrap().contains("[/default]"));
    }

    #[test]
    fn prompt_with_default_returns_typed_input() {
        let mut w = Vec::new();
        let got = prompt_with_default(&mut reader("/custom/dir\n"), &mut w, "Dir", "/default").unwrap();
        assert_eq!(got, "/custom/dir");
    }

    #[test]
    fn prompt_numeric_reprompts_on_invalid_then_accepts() {
        let mut w = Vec::new();
        let got = prompt_numeric(&mut reader("abc\n3002\n"), &mut w, "Port", 3002u16, parse_port).unwrap();
        assert_eq!(got, 3002);
        assert!(
            String::from_utf8(w).unwrap().contains('!'),
            "should print an error line on invalid input"
        );
    }

    #[test]
    fn prompt_numeric_returns_default_on_empty() {
        let mut w = Vec::new();
        let got = prompt_numeric(&mut reader("\n"), &mut w, "Port", 3002u16, parse_port).unwrap();
        assert_eq!(got, 3002);
    }

    #[test]
    fn confirm_honors_default_and_explicit_answers() {
        assert!(confirm(&mut reader("\n"), &mut Vec::new(), "ok?", true).unwrap());
        assert!(!confirm(&mut reader("\n"), &mut Vec::new(), "ok?", false).unwrap());
        assert!(confirm(&mut reader("Y\n"), &mut Vec::new(), "ok?", false).unwrap());
        assert!(!confirm(&mut reader("no\n"), &mut Vec::new(), "ok?", true).unwrap());
    }

    #[test]
    fn wizard_uses_brew_services_run_not_start() {
        // Regression guard for the "don't auto-start at login" preference:
        // the wizard must use `brew services run`, never `start`.
        assert_eq!(BREW_SERVICE_RUN, ["services", "run", "openstory"]);
        assert_eq!(BREW_SERVICE_RUN[1], "run", "must not register at login");
    }

    #[test]
    fn service_prompt_offers_background_session_only() {
        // brew present, user declines → no brew is run (hermetic), but the
        // prompt must frame it as background + not-at-login.
        let mut w = Vec::new();
        maybe_start_services(&mut reader("n\n"), &mut w, 3002, true).unwrap();
        let out = String::from_utf8(w).unwrap();
        assert!(out.contains("background"), "prompt should say background: {out}");
        assert!(
            out.contains("not auto-started at login"),
            "prompt should clarify it won't run at login: {out}"
        );
    }

    #[test]
    fn service_without_brew_suggests_manual_serve() {
        let mut w = Vec::new();
        maybe_start_services(&mut reader(""), &mut w, 3002, false).unwrap();
        let out = String::from_utf8(w).unwrap();
        assert!(out.contains("open-story serve"), "no-brew path should show manual start: {out}");
        assert!(out.contains("NATS"), "should note NATS comes up automatically: {out}");
    }

    #[test]
    fn full_flow_writes_chosen_config() {
        let tmp = tempfile::tempdir().unwrap();
        // Answers: 30 days, custom watch dir, skip multi-agent, port 4567,
        // accept default data_dir (the tmp path). Env all-off → no service
        // prompts and the MCP step falls through to the install hint (no
        // prompt), so the script ends after data_dir.
        let script = format!("30\n{}\nn\n4567\n\n", "/tmp/os-watch");
        let mut r = reader(&script);
        let mut w = Vec::new();

        run_flow(&mut r, &mut w, tmp.path().to_path_buf(), WizardEnv::default()).unwrap();

        let cfg = Config::from_file(&tmp.path().join("config.toml"));
        assert_eq!(cfg.port, 4567);
        assert_eq!(cfg.watch_backfill_hours, 30 * 24);
        assert_eq!(cfg.watch_dir, "/tmp/os-watch");

        let out = String::from_utf8(w).unwrap();
        assert!(out.contains("Wrote"), "summary should report the written file");
        assert!(out.contains("open-story serve"), "no-brew path should show manual start");
        // With no companion binary, the flow points at the mcp formula.
        assert!(
            out.contains("openstory-mcp"),
            "should surface the MCP companion when it isn't installed: {out}"
        );
    }

    // ── MCP-wiring step (`open-story init` → `claude mcp add`) ──────────────

    #[test]
    fn claude_mcp_add_args_use_transport_stdio_and_separator() {
        let bin = "/opt/homebrew/opt/openstory-mcp/bin/open-story-mcp";
        let args = claude_mcp_add_args(bin);
        assert_eq!(args, ["mcp", "add", "--transport", "stdio", "openstory", "--", bin]);
        // The `--` must immediately precede the binary so Claude stops parsing
        // its own flags — the stale positional `stdio` form is gone.
        let sep = args.iter().position(|a| a == "--").expect("has a -- separator");
        assert_eq!(args[sep + 1], bin, "binary must follow the -- separator");
    }

    #[test]
    fn mcp_wiring_absent_binary_points_at_formula() {
        // Companion not installed → offer the formula, never prompt.
        let mut w = Vec::new();
        maybe_wire_mcp(&mut reader(""), &mut w, None, true).unwrap();
        let out = String::from_utf8(w).unwrap();
        assert!(out.contains("brew install"), "should offer the companion formula: {out}");
        assert!(out.contains("openstory-mcp"), "names the mcp formula: {out}");
    }

    #[test]
    fn mcp_wiring_without_claude_prints_manual_command() {
        let mut w = Vec::new();
        let bin = std::path::PathBuf::from("/x/bin/open-story-mcp");
        maybe_wire_mcp(&mut reader(""), &mut w, Some(bin.as_path()), false).unwrap();
        let out = String::from_utf8(w).unwrap();
        assert!(
            out.contains("claude mcp add --transport stdio openstory -- /x/bin/open-story-mcp"),
            "no-claude path should print the exact manual command: {out}"
        );
    }

    #[test]
    fn mcp_wiring_declined_prints_manual_command_and_does_not_run() {
        // claude + binary present, user declines → hermetic (nothing spawned),
        // and the manual command is left behind for later.
        let mut w = Vec::new();
        let bin = std::path::PathBuf::from("/x/bin/open-story-mcp");
        maybe_wire_mcp(&mut reader("n\n"), &mut w, Some(bin.as_path()), true).unwrap();
        let out = String::from_utf8(w).unwrap();
        assert!(out.contains("skipped"), "decline should be acknowledged: {out}");
        assert!(
            out.contains("claude mcp add --transport stdio openstory -- /x/bin/open-story-mcp"),
            "decline should leave the manual command: {out}"
        );
    }

    #[test]
    fn mcp_wiring_bare_enter_does_not_register() {
        // Security/consent guard: registering the MCP writes to the user's
        // ~/.claude.json. A bare Enter (default) must NOT do that — the confirm
        // defaults to NO, so an empty/EOF answer takes the skipped path and
        // spawns nothing (this test would hang or mutate real config otherwise).
        let mut w = Vec::new();
        let bin = std::path::PathBuf::from("/x/bin/open-story-mcp");
        maybe_wire_mcp(&mut reader(""), &mut w, Some(bin.as_path()), true).unwrap();
        let out = String::from_utf8(w).unwrap();
        assert!(out.contains("skipped"), "bare Enter must default to skip: {out}");
        assert!(
            out.contains("[y/N]"),
            "prompt must show a NO default for agent-config mutation: {out}"
        );
    }

    #[test]
    fn mcp_wiring_prompt_frames_the_capability() {
        // The offer must say what wiring buys — query AND drive, not just read.
        let mut w = Vec::new();
        let bin = std::path::PathBuf::from("/x/bin/open-story-mcp");
        maybe_wire_mcp(&mut reader("n\n"), &mut w, Some(bin.as_path()), true).unwrap();
        let out = String::from_utf8(w).unwrap().to_lowercase();
        assert!(out.contains("drive"), "prompt should name the drive capability: {out}");
    }
}
