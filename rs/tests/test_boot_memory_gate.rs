//! B-10: the boot-memory gate as a container integration test
//! (docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! The production image boots against a synthetic store inside a 512 MiB
//! cgroup, with the bus on a NATS sidecar, and must reach serving without
//! an OOM kill or a restart, with the read model bounded below the session
//! count, the cgroup limit visible on health, jemalloc as the allocator,
//! and no `memory_pressure` finding in the verdict. The store is sized so
//! the pre-B-09 node (every projection resident, ≈ 0.9 B resident per JSONL
//! byte) would need more than the limit; the numbers are in the loop log.
//!
//! Opt-in like the other container tests: needs Docker and the image from
//! `just docker-build`; runs under `just test-container`; skips with a
//! message when Docker is not running. The fixture comes from the B-00
//! harness (`scripts/boot_memory.py --build-only`) in a temp dir, or from
//! `BMGATE_FIXTURE_DIR` to reuse one across runs. Containers are named
//! `bmgate-*` on their own network and removed at the end; the node's SQLite
//! is never shared between two running boots (the two boots here are
//! sequential: the first ingests the JSONL, the second is the measured
//! DB-present boot the fleet's restarts look like).
//!
//! Run with: cargo test -p open-story --test test_boot_memory_gate -- --nocapture

mod helpers;

use std::path::PathBuf;
use std::time::Instant;

use helpers::boot_gate::{
    assert_gate, boot, build_fixture, docker, docker_running, BootOpts, FixtureShape, Stack,
};

const IMAGE: &str = "open-story:test";
/// The cgroup limit the node boots under. Docker's `--memory 512m` is MiB.
const MEMORY_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
/// The fixture: 150 sessions, 8 of them large (4000 events), 0.8 GB of JSONL.
const SHAPE: FixtureShape = FixtureShape {
    sessions: 150,
    large: 8,
    large_events: 4000,
    total_gb: "0.8",
};

mod when_the_production_container_boots_inside_512_mib {
    use super::*;

    /// Two sequential boots on one store: the first ingests the JSONL into
    /// SQLite, the second is the DB-present restart the fleet's nodes make.
    /// Both must pass the gate.
    #[tokio::test]
    async fn it_serves_with_the_read_model_bounded_and_no_oom_kill() {
        if !docker_running() {
            eprintln!("skipping: Docker is not running (this gate needs it)");
            return;
        }
        assert!(
            docker(&["image", "inspect", IMAGE]).is_ok(),
            "image {IMAGE} not found — build it with `just docker-build`"
        );

        let keep = std::env::var_os("BMGATE_FIXTURE_DIR").map(PathBuf::from);
        let tmp = tempfile::tempdir().unwrap();
        let root = keep
            .clone()
            .unwrap_or_else(|| tmp.path().join("boot-memory-gate"));
        let t_fixture = Instant::now();
        let fixture = build_fixture(&root, &SHAPE);
        eprintln!(
            "fixture: {} sessions, {} events, {:.2} GB JSONL at {} ({:?}; {})",
            fixture.files,
            fixture.events,
            fixture.jsonl_bytes as f64 / 1e9,
            root.display(),
            t_fixture.elapsed(),
            if keep.is_some() { "kept" } else { "temp" }
        );

        let stack = Stack {
            prefix: "bmgate",
            tag: format!(
                "{}-{}",
                std::process::id(),
                t_fixture.elapsed().as_nanos() % 100_000
            ),
        };
        stack.up_nats();
        let memory = format!("{}m", MEMORY_LIMIT_BYTES >> 20);

        let opts = BootOpts {
            image: IMAGE,
            memory: &memory,
            env: vec![],
            drain_via: None,
        };
        let first = boot(&stack, &fixture, &opts).await;
        assert_gate("boot 1 (ingest)", &first, &fixture, MEMORY_LIMIT_BYTES);
        let second = boot(&stack, &fixture, &opts).await;
        assert_gate("boot 2 (DB present)", &second, &fixture, MEMORY_LIMIT_BYTES);
    }
}
