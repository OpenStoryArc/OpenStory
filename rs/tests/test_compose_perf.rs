//! Container-sized performance tests — Break the Bus.
//!
//! These tests generate massive synthetic transcript data, mount it into
//! resource-constrained containers, and stress the watcher→NATS→ingest
//! pipeline to find breaking points at small/medium/large container tiers.
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_compose_perf -- --ignored --nocapture
//!
//! Tests are #[ignore] because they're slow (30s–5min+) and resource-heavy.

mod helpers;

use helpers::synth;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use testcontainers::compose::DockerCompose;

// ── Container sizing tiers ──────────────────────────────────────────

#[derive(Debug, Clone)]
struct SizingTier {
    name: &'static str,
    server_cpu: &'static str,
    server_memory: &'static str,
    nats_cpu: &'static str,
    nats_memory: &'static str,
}

const SMALL: SizingTier = SizingTier {
    name: "Small",
    server_cpu: "0.5",
    server_memory: "256M",
    nats_cpu: "0.25",
    nats_memory: "128M",
};

const MEDIUM: SizingTier = SizingTier {
    name: "Medium",
    server_cpu: "1.0",
    server_memory: "512M",
    nats_cpu: "0.5",
    nats_memory: "256M",
};

const LARGE: SizingTier = SizingTier {
    name: "Large",
    server_cpu: "2.0",
    server_memory: "1G",
    nats_cpu: "1.0",
    nats_memory: "512M",
};

// ── Pipeline result ─────────────────────────────────────────────────

#[derive(Debug)]
struct PipelineResult {
    tier: String,
    sessions_generated: usize,
    events_generated: usize,
    events_ingested: usize,
    elapsed: Duration,
    throughput_eps: f64,
    data_loss_pct: f64,
    healthy: bool,
}

impl PipelineResult {
    fn print(&self) {
        eprintln!("\n  ╔══ Pipeline Result: {} ══╗", self.tier);
        eprintln!("  ║ Sessions generated: {}", self.sessions_generated);
        eprintln!("  ║ Events generated:   {}", self.events_generated);
        eprintln!("  ║ Events ingested:    {}", self.events_ingested);
        eprintln!("  ║ Elapsed:            {:.1}s", self.elapsed.as_secs_f64());
        eprintln!("  ║ Throughput:         {:.0} events/s", self.throughput_eps);
        eprintln!("  ║ Data loss:          {:.1}%", self.data_loss_pct);
        eprintln!("  ║ Healthy:            {}", self.healthy);
        eprintln!("  ╚════════════════════════════╝");
    }
}

// ── Latency stats ───────────────────────────────────────────────────

#[derive(Debug)]
struct LatencyStats {
    count: u64,
    total_ms: f64,
    min_ms: f64,
    max_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    errors: u64,
}

fn compute_stats(latencies: &mut Vec<f64>, errors: u64) -> LatencyStats {
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let count = latencies.len() as u64;
    if count == 0 {
        return LatencyStats {
            count: 0,
            total_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            errors,
        };
    }
    let total_ms: f64 = latencies.iter().sum();
    let p = |pct: f64| -> f64 {
        let idx = ((pct / 100.0) * (count as f64 - 1.0)).round() as usize;
        latencies[idx.min(count as usize - 1)]
    };
    LatencyStats {
        count,
        total_ms,
        min_ms: latencies[0],
        max_ms: latencies[count as usize - 1],
        p50_ms: p(50.0),
        p95_ms: p(95.0),
        p99_ms: p(99.0),
        errors,
    }
}

fn print_stats(label: &str, stats: &LatencyStats, elapsed: Duration) {
    let throughput = stats.count as f64 / elapsed.as_secs_f64();
    eprintln!("\n  === {label} ===");
    eprintln!("  Requests:    {}", stats.count);
    eprintln!("  Errors:      {}", stats.errors);
    eprintln!("  Duration:    {:.1}s", elapsed.as_secs_f64());
    eprintln!("  Throughput:  {throughput:.0} req/s");
    eprintln!("  Latency p50: {:.1}ms", stats.p50_ms);
    eprintln!("  Latency p95: {:.1}ms", stats.p95_ms);
    eprintln!("  Latency p99: {:.1}ms", stats.p99_ms);
    eprintln!("  Latency min: {:.1}ms", stats.min_ms);
    eprintln!("  Latency max: {:.1}ms", stats.max_ms);
    if stats.errors > 0 {
        let error_rate = stats.errors as f64 / (stats.count + stats.errors) as f64 * 100.0;
        eprintln!("  Error rate:  {error_rate:.1}%");
    }
}

// ── Compose infrastructure ──────────────────────────────────────────

fn perf_compose_file() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/docker-compose.perf.yml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn to_docker_path(path: &std::path::Path) -> String {
    let canonical = path.canonicalize().expect("canonicalize path");
    let s = canonical.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

async fn start_perf_stack(tier: &SizingTier, fixture_dir: &std::path::Path) -> (DockerCompose, u16) {
    // Touch fixture files for fresh mtimes (watcher skips old files)
    let now = filetime::FileTime::now();
    if let Ok(entries) = std::fs::read_dir(fixture_dir) {
        for entry in entries.flatten() {
            let _ = filetime::set_file_mtime(&entry.path(), now);
        }
    }

    let fixture_path = to_docker_path(fixture_dir);

    eprintln!("\n  Starting {tier:?}");
    eprintln!("  Fixture dir: {fixture_path}");

    let mut compose = DockerCompose::with_local_client(&[perf_compose_file()])
        .with_env("FIXTURE_DIR", &fixture_path)
        .with_env("SERVER_CPU_LIMIT", tier.server_cpu)
        .with_env("SERVER_MEMORY_LIMIT", tier.server_memory)
        .with_env("NATS_CPU_LIMIT", tier.nats_cpu)
        .with_env("NATS_MEMORY_LIMIT", tier.nats_memory)
        .with_wait(false);

    compose.up().await.expect("docker compose up failed");

    let server = compose.service("server").expect("server service not found");
    let port = server
        .get_host_port_ipv4(3002)
        .await
        .expect("failed to get server port");

    // Wait for HTTP readiness
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..60 {
        if reqwest::get(&url).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    eprintln!("  Server ready on port {port}");
    (compose, port)
}

// ── Polling helpers ─────────────────────────────────────────────────

/// Poll until at least `expected` sessions appear, or timeout.
async fn wait_for_n_sessions(port: u16, expected: usize, timeout: Duration) -> Vec<Value> {
    let url = format!("http://localhost:{port}/api/sessions");
    let start = Instant::now();
    let mut last_count = 0;

    while start.elapsed() < timeout {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(sessions) = resp.json::<Vec<Value>>().await {
                last_count = sessions.len();
                if last_count >= expected {
                    return sessions;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    eprintln!("  TIMEOUT: expected {expected} sessions, got {last_count} after {:.0}s", timeout.as_secs_f64());
    // Return what we have
    if let Ok(resp) = reqwest::get(&url).await {
        resp.json::<Vec<Value>>().await.unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Poll until a session has at least `expected` view-records, or timeout.
async fn wait_for_n_records(port: u16, session_id: &str, expected: usize, timeout: Duration) -> usize {
    let url = format!("http://localhost:{port}/api/sessions/{session_id}/view-records");
    let start = Instant::now();
    let mut last_count = 0;

    while start.elapsed() < timeout {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(records) = resp.json::<Vec<Value>>().await {
                last_count = records.len();
                if last_count >= expected {
                    return last_count;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    last_count
}

/// Count total view-records across all sessions.
async fn count_total_records(port: u16) -> usize {
    let sessions_url = format!("http://localhost:{port}/api/sessions");
    let sessions: Vec<Value> = match reqwest::get(&sessions_url).await {
        Ok(resp) => resp.json().await.unwrap_or_default(),
        Err(_) => return 0,
    };

    let mut total = 0;
    for session in &sessions {
        if let Some(id) = session["session_id"].as_str() {
            let url = format!("http://localhost:{port}/api/sessions/{id}/view-records");
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(records) = resp.json::<Vec<Value>>().await {
                    total += records.len();
                }
            }
        }
    }
    total
}

/// Check if the server is still healthy (responds to HTTP).
async fn is_healthy(port: u16) -> bool {
    let url = format!("http://localhost:{port}/api/sessions");
    reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ══════════════════════════════════════════════════════════════════════
// BACKFILL THROUGHPUT TESTS
// ══════════════════════════════════════════════════════════════════════
//
// Pre-populate JSONL files, start containers, measure how fast the
// watcher→NATS→ingest pipeline processes the full backlog.

async fn run_backfill_test(
    tier: &SizingTier,
    sessions: usize,
    events_per_session: usize,
    payload_size: usize,
    timeout: Duration,
) -> PipelineResult {
    let total_events = sessions * events_per_session;
    eprintln!("\n  ══ Backfill: {sessions} sessions × {events_per_session} events = {total_events} total ══");

    // Generate fixtures
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    synth::generate_fixture_dir(tmp.path(), sessions, events_per_session, payload_size);
    eprintln!("  Generated {sessions} session files in {:?}", tmp.path());

    // Start stack
    let (_compose, port) = start_perf_stack(tier, tmp.path()).await;

    // Wait for all sessions to appear
    let start = Instant::now();
    let found_sessions = wait_for_n_sessions(port, sessions, timeout).await;
    let sessions_found = found_sessions.len();

    // Count total ingested records across all sessions
    let total_ingested = count_total_records(port).await;
    let elapsed = start.elapsed();

    let healthy = is_healthy(port).await;
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        total_ingested as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };
    let data_loss = if total_events > 0 {
        (1.0 - (total_ingested as f64 / total_events as f64)) * 100.0
    } else {
        0.0
    };

    let result = PipelineResult {
        tier: format!("{} ({sessions}×{events_per_session})", tier.name),
        sessions_generated: sessions,
        events_generated: total_events,
        events_ingested: total_ingested,
        elapsed,
        throughput_eps: throughput,
        data_loss_pct: data_loss.max(0.0), // clamp negative (more ingested than generated due to progress dedup)
        healthy,
    };
    result.print();

    // Log session-level detail
    eprintln!("\n  Sessions found: {sessions_found}/{sessions}");
    if sessions_found > 0 && sessions_found <= 20 {
        for s in &found_sessions {
            let id = s["session_id"].as_str().unwrap_or("?");
            let count = s.get("event_count").and_then(|v| v.as_u64()).unwrap_or(0);
            eprintln!("    {id}: {count} events");
        }
    }

    result
}

/// Small tier backfill: 5 sessions × 100 events = 500 total.
#[tokio::test]
#[ignore]
async fn perf_backfill_small() {
    let result = run_backfill_test(&SMALL, 5, 100, 200, Duration::from_secs(60)).await;
    assert!(result.healthy, "server should still be healthy");
    assert!(
        result.data_loss_pct < 10.0,
        "data loss should be <10%, got {:.1}%",
        result.data_loss_pct
    );
}

/// Medium tier backfill: 20 sessions × 500 events = 10K total.
#[tokio::test]
#[ignore]
async fn perf_backfill_medium() {
    let result = run_backfill_test(&MEDIUM, 20, 500, 200, Duration::from_secs(120)).await;
    assert!(result.healthy, "server should still be healthy");
    assert!(
        result.data_loss_pct < 10.0,
        "data loss should be <10%, got {:.1}%",
        result.data_loss_pct
    );
}

/// Large tier backfill: 100 sessions × 1000 events = 100K total.
#[tokio::test]
#[ignore]
async fn perf_backfill_large() {
    let result = run_backfill_test(&LARGE, 100, 1000, 200, Duration::from_secs(300)).await;
    assert!(result.healthy, "server should still be healthy");
    assert!(
        result.data_loss_pct < 15.0,
        "data loss should be <15%, got {:.1}%",
        result.data_loss_pct
    );
}

// ══════════════════════════════════════════════════════════════════════
// BREAK-FINDER: Ramp until failure
// ══════════════════════════════════════════════════════════════════════
//
// Progressively double the load until the system breaks. Each round
// generates fresh data and measures ingestion. Failure = data loss >5%,
// health check fails, timeout >5min, or container OOM (exit 137).

#[derive(Debug)]
struct BreakRound {
    round: usize,
    sessions: usize,
    events_per_session: usize,
    total_events: usize,
    events_ingested: usize,
    elapsed: Duration,
    throughput_eps: f64,
    data_loss_pct: f64,
    healthy: bool,
    broke: bool,
}

async fn run_break_finder(tier: &SizingTier) -> Vec<BreakRound> {
    eprintln!("\n  ╔══════════════════════════════════════╗");
    eprintln!("  ║  BREAK FINDER: {} tier", tier.name);
    eprintln!("  ║  CPU: {} server / {} NATS", tier.server_cpu, tier.nats_cpu);
    eprintln!("  ║  RAM: {} server / {} NATS", tier.server_memory, tier.nats_memory);
    eprintln!("  ╚══════════════════════════════════════╝");

    let mut rounds = Vec::new();
    let mut sessions = 5usize;
    let mut events_per = 100usize;
    let max_timeout = Duration::from_secs(300);

    for round in 1..=10 {
        let total = sessions * events_per;
        eprintln!("\n  ── Round {round}: {sessions} sessions × {events_per} events = {total} ──");

        let tmp = tempfile::TempDir::new().unwrap();
        synth::generate_fixture_dir(tmp.path(), sessions, events_per, 200);

        let compose_result = start_perf_stack(tier, tmp.path()).await;
        let (_compose, port) = compose_result;

        let start = Instant::now();

        // Wait for sessions with proportional timeout (min 30s, max 300s)
        let timeout = Duration::from_secs((30 + total as u64 / 50).min(300));
        let found = wait_for_n_sessions(port, sessions, timeout).await;
        let sessions_found = found.len();

        let total_ingested = count_total_records(port).await;
        let elapsed = start.elapsed();
        let healthy = is_healthy(port).await;

        let throughput = if elapsed.as_secs_f64() > 0.0 {
            total_ingested as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };
        let data_loss = if total > 0 {
            ((1.0 - (total_ingested as f64 / total as f64)) * 100.0).max(0.0)
        } else {
            0.0
        };

        let broke = data_loss > 5.0 || !healthy || elapsed > max_timeout;

        let result = BreakRound {
            round,
            sessions,
            events_per_session: events_per,
            total_events: total,
            events_ingested: total_ingested,
            elapsed,
            throughput_eps: throughput,
            data_loss_pct: data_loss,
            healthy,
            broke,
        };

        eprintln!("  Round {round}: {sessions_found}/{sessions} sessions, {total_ingested}/{total} events, {:.0} e/s, {:.1}% loss, healthy={}",
            throughput, data_loss, healthy);

        rounds.push(result);

        if broke {
            eprintln!("  *** BROKE at round {round} ***");
            break;
        }

        // Ramp: ~3x per round (double sessions, 1.5x events)
        sessions = (sessions as f64 * 2.0) as usize;
        events_per = (events_per as f64 * 1.5) as usize;
    }

    // Print summary table
    eprintln!("\n  ══ Break Finder Summary: {} ══", tier.name);
    eprintln!("  {:>5} {:>8} {:>8} {:>8} {:>10} {:>8} {:>8} {:>7} {:>5}",
        "Round", "Sessions", "Evts/S", "Total", "Ingested", "Time(s)", "E/s", "Loss%", "OK?");
    for r in &rounds {
        eprintln!("  {:>5} {:>8} {:>8} {:>8} {:>10} {:>8.1} {:>8.0} {:>7.1} {:>5}",
            r.round, r.sessions, r.events_per_session, r.total_events,
            r.events_ingested, r.elapsed.as_secs_f64(), r.throughput_eps,
            r.data_loss_pct, if r.healthy { "yes" } else { "NO" });
    }

    rounds
}

/// Break the small tier: 0.5 CPU / 256M.
#[tokio::test]
#[ignore]
async fn perf_break_small() {
    let rounds = run_break_finder(&SMALL).await;
    assert!(!rounds.is_empty(), "should complete at least one round");
    // First round should succeed
    assert!(rounds[0].healthy, "round 1 should be healthy");
    assert!(rounds[0].data_loss_pct < 10.0, "round 1 data loss < 10%");
}

/// Break the medium tier: 1 CPU / 512M.
#[tokio::test]
#[ignore]
async fn perf_break_medium() {
    let rounds = run_break_finder(&MEDIUM).await;
    assert!(!rounds.is_empty(), "should complete at least one round");
    assert!(rounds[0].healthy, "round 1 should be healthy");
    // Medium should survive at least 2 rounds
    if rounds.len() >= 2 {
        assert!(rounds[1].healthy, "round 2 should be healthy on medium");
    }
}

/// Break the large tier: 2 CPU / 1G.
#[tokio::test]
#[ignore]
async fn perf_break_large() {
    let rounds = run_break_finder(&LARGE).await;
    assert!(!rounds.is_empty(), "should complete at least one round");
    assert!(rounds[0].healthy, "round 1 should be healthy");
    // Large should survive at least 3 rounds
    if rounds.len() >= 3 {
        assert!(rounds[2].healthy, "round 3 should be healthy on large");
    }
}

// ══════════════════════════════════════════════════════════════════════
// PAYLOAD SIZE RAMP
// ══════════════════════════════════════════════════════════════════════
//
// Vary payload size to find where large tool results cause issues.

/// Payload size ramp: 5 sessions × 100 events at 200B, 2KB, 20KB, 200KB.
#[tokio::test]
#[ignore]
async fn perf_payload_ramp() {
    let sizes = [200, 2_000, 20_000, 200_000];
    let sessions = 5;
    let events_per = 100;

    eprintln!("\n  ══ Payload Size Ramp (Medium tier) ══");

    for size in sizes {
        let result = run_backfill_test(
            &MEDIUM,
            sessions,
            events_per,
            size,
            Duration::from_secs(120),
        )
        .await;

        let size_label = if size >= 1000 {
            format!("{}KB", size / 1000)
        } else {
            format!("{}B", size)
        };
        eprintln!("  Payload {size_label}: {:.0} e/s, {:.1}% loss, healthy={}",
            result.throughput_eps, result.data_loss_pct, result.healthy);
    }
}

// ══════════════════════════════════════════════════════════════════════
// NFR REPORT
// ══════════════════════════════════════════════════════════════════════
//
// Run all tiers and print a consolidated NFR table.

/// Full NFR report: runs small, medium, large backfill + break tests.
/// This is the slowest test — runs all three tiers sequentially.
#[tokio::test]
#[ignore]
async fn perf_nfr_report() {
    eprintln!("\n  ╔══════════════════════════════════════════════╗");
    eprintln!("  ║     NFR REPORT — Container Performance       ║");
    eprintln!("  ╚══════════════════════════════════════════════╝");

    // Backfill tests
    let small = run_backfill_test(&SMALL, 5, 100, 200, Duration::from_secs(60)).await;
    let medium = run_backfill_test(&MEDIUM, 20, 500, 200, Duration::from_secs(120)).await;
    let large = run_backfill_test(&LARGE, 100, 1000, 200, Duration::from_secs(300)).await;

    // Break tests
    let small_break = run_break_finder(&SMALL).await;
    let medium_break = run_break_finder(&MEDIUM).await;
    let large_break = run_break_finder(&LARGE).await;

    // Find breaking points
    let break_point = |rounds: &[BreakRound]| -> String {
        if let Some(r) = rounds.iter().find(|r| r.broke) {
            format!("Broke at {} events", r.total_events)
        } else {
            let last = rounds.last().unwrap();
            format!("Survived {} events", last.total_events)
        }
    };

    let max_throughput = |rounds: &[BreakRound]| -> f64 {
        rounds
            .iter()
            .filter(|r| !r.broke)
            .map(|r| r.throughput_eps)
            .fold(0.0f64, f64::max)
    };

    // Print consolidated table
    eprintln!("\n  ═══════════════════════════════════════════════════════════════════════════════");
    eprintln!("  NON-FUNCTIONAL REQUIREMENTS — Container Sizing Recommendations");
    eprintln!("  ═══════════════════════════════════════════════════════════════════════════════");
    eprintln!();
    eprintln!("  {:>8} {:>6} {:>6} {:>10} {:>10} {:>12} {:>30}",
        "Tier", "CPU", "RAM", "Backfill", "Throughput", "Data Loss", "Breaking Point");
    eprintln!("  {:>8} {:>6} {:>6} {:>10} {:>10} {:>12} {:>30}",
        "────────", "──────", "──────", "──────────", "──────────", "────────────", "──────────────────────────────");
    eprintln!("  {:>8} {:>6} {:>6} {:>10} {:>10.0} {:>11.1}% {:>30}",
        "Small", "0.5", "256M", format!("{:.0}s", small.elapsed.as_secs_f64()),
        max_throughput(&small_break), small.data_loss_pct,
        break_point(&small_break));
    eprintln!("  {:>8} {:>6} {:>6} {:>10} {:>10.0} {:>11.1}% {:>30}",
        "Medium", "1.0", "512M", format!("{:.0}s", medium.elapsed.as_secs_f64()),
        max_throughput(&medium_break), medium.data_loss_pct,
        break_point(&medium_break));
    eprintln!("  {:>8} {:>6} {:>6} {:>10} {:>10.0} {:>11.1}% {:>30}",
        "Large", "2.0", "1G", format!("{:.0}s", large.elapsed.as_secs_f64()),
        max_throughput(&large_break), large.data_loss_pct,
        break_point(&large_break));
    eprintln!();
    eprintln!("  Recommendations:");
    eprintln!("    Small  (0.5 CPU, 256M): Dev / single-agent — up to ~500 events");
    eprintln!("    Medium (1.0 CPU, 512M): Multi-agent dev — up to ~10K events");
    eprintln!("    Large  (2.0 CPU, 1G):   Production / team — up to ~100K events");
    eprintln!();
    eprintln!("  Known bottlenecks:");
    eprintln!("    1. RwLock contention in ingest_events() — single writer, exclusive lock");
    eprintln!("    2. Disk I/O — 2 synchronous file writes per event");
    eprintln!("    3. mpsc(256) buffer — backpressure when consumer can't keep up");
    eprintln!("    4. Memory growth — every Value stays in RAM (no eviction)");
    eprintln!("    5. JetStream 1GB — DiscardOld evicts under sustained load");
    eprintln!("  ═══════════════════════════════════════════════════════════════════════════════");
}
