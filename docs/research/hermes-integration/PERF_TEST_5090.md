# Performance Test: Hermes + OpenStory on a 5090

*The real stress test. A local model responding in milliseconds, not seconds.*

---

## Why this matters

All our measurements so far are against Anthropic's API over the network:
- LLM latency: 2-5 seconds per response
- Write frequency: 0.3 writes/sec
- Gap between writes: 2-3.5 seconds

With a **5090 running a local model** (vLLM, SGLang, or llama.cpp), the LLM latency drops to **50-200ms**. This means:
- Write frequency: **5-20 writes/sec** sustained
- Gap between writes: **50-200ms**
- Tool execution becomes the bottleneck, not the LLM

This is the scenario where our snapshot-diff watcher would be genuinely stressed.

## Hardware

- GPU: NVIDIA RTX 5090 (32GB VRAM)
- Recommended models:
  - **NousResearch/Hermes-3-Llama-3.1-8B** — the natural choice (it's Nous's own model, 8B fits easily)
  - **Qwen/Qwen2.5-Coder-7B-Instruct** — fast, good at tool calling
  - **mistralai/Mistral-7B-Instruct-v0.3** — reliable baseline
- Serving: vLLM or SGLang (both support OpenAI-compatible API)

## Setup

### 1. Start the local model server

```bash
# vLLM (recommended — mature, well-tested with Hermes)
pip install vllm
python -m vllm.entrypoints.openai.api_server \
  --model NousResearch/Hermes-3-Llama-3.1-8B \
  --host 0.0.0.0 --port 8000 \
  --max-model-len 8192 \
  --gpu-memory-utilization 0.9

# Or SGLang (faster for small models)
pip install sglang
python -m sglang.launch_server \
  --model NousResearch/Hermes-3-Llama-3.1-8B \
  --port 8000
```

### 2. Configure Hermes to use the local model

```bash
# Run Hermes against the local server
hermes chat \
  -q "Read all Python files in this directory and summarize each one" \
  --provider openai \
  -m NousResearch/Hermes-3-Llama-3.1-8B

# Or set in config.yaml:
# model: NousResearch/Hermes-3-Llama-3.1-8B
# provider: openai
# base_url: http://localhost:8000/v1
```

### 3. Start OpenStory with the Hermes watcher

```bash
OPEN_STORY_HERMES_WATCH_DIR=~/.hermes/sessions just serve
```

### 4. Start the write monitor

```bash
python3 scripts/hermes_write_timeline.py --live ~/.hermes/sessions/
# (needs the --live flag added — currently only reads post-hoc)
```

## Test Protocol

### Test A: Single session, sustained tool use

**Goal:** Measure write frequency and watcher reliability with a fast local model.

```bash
# Create a project with many small files for Hermes to process
mkdir -p /tmp/perf-test && cd /tmp/perf-test
for i in $(seq 1 20); do
  echo "def func_$i(): return $i" > "module_$i.py"
done
echo "# Project with 20 modules" > README.md

# Run Hermes with a task that exercises many tool calls
hermes chat \
  -q "Read every Python file in this directory. For each one, add a docstring. Then run all of them to verify they still work." \
  --provider openai \
  -m NousResearch/Hermes-3-Llama-3.1-8B
```

**Expected:** 40+ tool calls (20 reads + 20 writes), potentially 40+ file writes in rapid succession.

**Measurements:**
- Write frequency (from the monitor script)
- Detection rate (from the snapshot watcher)
- Latency distribution (from the perf tests)
- File size growth curve
- Any missed messages

### Test B: Concurrent sessions (gateway mode simulation)

**Goal:** Multiple Hermes sessions hitting the same local model.

```bash
# Terminal 1
hermes chat -q "List all files and read README.md" --provider openai -m Hermes-3-Llama-3.1-8B

# Terminal 2 (simultaneously)
hermes chat -q "Write a hello.py and run it" --provider openai -m Hermes-3-Llama-3.1-8B

# Terminal 3 (simultaneously)
hermes chat -q "Search for TODO comments in all files" --provider openai -m Hermes-3-Llama-3.1-8B
```

**Expected:** 3 session files being written concurrently, each at 5-20 writes/sec.

### Test C: Sustained 10-minute session

**Goal:** Find stability issues over time (memory leaks, file handle exhaustion, etc.)

```bash
# A long, complex task that keeps the agent busy
hermes chat \
  -q "Build a complete Python project: create a calculator module with add/sub/mul/div, write tests for it, run the tests, fix any failures, then add a CLI interface." \
  --provider openai \
  -m NousResearch/Hermes-3-Llama-3.1-8B
```

**Expected:** 100+ messages, multiple compression cycles, session ID changes.

### Test D: Throughput ceiling

**Goal:** Find the absolute write rate where the watcher breaks.

```bash
# Use the Rust stress tests instead of real Hermes
cargo test --test test_hermes_snapshot_watcher test_write_rate_sweep -- --nocapture
cargo test --test test_hermes_snapshot_watcher test_fast_model_simulation -- --nocapture
```

**Compare:** synthetic ceiling vs real Hermes ceiling.

## What to measure

| Metric | How | Tool |
|---|---|---|
| Write frequency | Monitor session file mtime | `monitor_writes.py` |
| Write latency (Hermes side) | Time from LLM response to fsync+rename | Hermes logs |
| Detection rate | Events detected / events written | Rust stress tests |
| Parse latency | Time to read + parse + diff | Rust perf tests |
| E2E latency | File write → CloudEvent in dashboard | OpenStory server logs |
| Memory | Process RSS over time | `ps` or `/proc/self/status` |
| File handle count | `lsof -p <pid> | wc -l` | Shell |
| CPU utilization | Watcher process CPU% | `top` |

## Success criteria

| Scenario | Target |
|---|---|
| Single session, fast model | ≥95% detection at 10 writes/sec |
| Concurrent sessions (3x) | ≥90% detection per session |
| 10-minute sustained | Zero memory leaks, zero panics |
| Throughput ceiling | Know the exact rate where detection drops below 80% |

## The question this answers

**"Can OpenStory observe a Hermes agent running on a 5090 with a local model without missing events or degrading performance?"**

If yes: the snapshot-diff watcher ships as the primary ingestion path, no plugin needed.

If no at high rates: we add a configurable poll interval and document the recommended setting per deployment scenario (API model: 1s, local model: 100ms).

## Network topology options

### Option 1: Same machine (simplest)
```
[5090 GPU] → [vLLM :8000] → [Hermes Agent] → [session_*.json]
                                                     ↓
                                              [OpenStory watcher]
                                                     ↓
                                              [Dashboard :3002]
```

### Option 2: Separate machines (production-like)
```
Machine A (GPU):
  [5090] → [vLLM :8000]

Machine B (Dev):
  [Hermes Agent] → [session_*.json] → [OpenStory]
  (calls Machine A's vLLM endpoint)
```

### Option 3: Container deployment
```
[Docker: vLLM + 5090 passthrough] → port 8000
[Docker: Hermes Agent]            → shared volume: /sessions/
[Docker: OpenStory]               → watches /sessions/
```

Option 2 is closest to real use. The 5090 just serves the model; Hermes and OpenStory run locally on the dev machine. The LLM latency is network round-trip to Machine A (~1-10ms on LAN), so response times are still dominated by the model's generation speed.

---

## Running the stress tests without a GPU

The Rust stress tests (`test_fast_model_simulation`, `test_write_rate_sweep`) simulate the write patterns a fast model would produce WITHOUT needing a GPU. They write files at controlled rates and measure the watcher's behavior.

```bash
# Run all stress tests
cargo test --test test_hermes_snapshot_watcher -- --nocapture

# Run the 5-minute sustained test (opt-in, takes time)
cargo test --test test_hermes_snapshot_watcher test_sustained_load -- --nocapture --ignored
```

The GPU test is the *real-world validation* of what the synthetic stress tests predict. If the synthetic test says "breaks at 50 writes/sec" and the real Hermes-on-5090 produces 15 writes/sec, we have 3x headroom and we're safe.
