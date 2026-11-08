<!--
CONCEPT: manual main-Mac and helper bring-up, independent of unfinished Rust integration.
WHY: WP-1 R1.1-R1.7; D-08 requires three physical machines; O-14 pins build 8665.
DEPENDS ON: b8665 official binaries, local GGUF, private LAN; docs/network.md.
AFFECTS: WP-2 load fixtures, WP-4 launcher, WP-5 calibration, WP-11 model acceptance.
-->
# Manual bring-up (P4)

Status: procedure prepared, **not yet executed end to end**. See findings.md for actual evidence.
Main: Preston's M3 Air, 16 GB RAM. No successful inference or Checkpoint A is claimed.

## 1. Install and inventory

Use the macOS arm64 asset from the official [b8665 release](https://github.com/ggml-org/llama.cpp/releases/tag/b8665).
Extract the entire archive together so its dynamic libraries remain beside the executables.
Use the corresponding official b8665 asset on each helper. Do not substitute current Homebrew builds.
Put the extracted directory on PATH in each terminal. Record:

```bash
mkdir -p fixtures/llama models
llama-server --version > fixtures/llama/main-version.txt 2>&1
llama-server --list-devices > fixtures/llama/main-devices.txt 2>&1
llama-server --help > fixtures/llama/main-help.txt 2>&1
rpc-server --help > fixtures/llama/main-rpc-help.txt 2>&1
df -h .
```

Check the version actually says 8665. Use this Mac's reported Metal budget; do not
copy the M5's 18186 MiB or assume a fraction of physical RAM. Capture each helper's
version and devices with its own machine name. Inspect b8665 help before running
the commands below; flags are based on team observations, unverified on this Mac.

Keep power attached; run `caffeinate -dims` in a separate main-Mac terminal during tests.
Follow [network.md](network.md) for the private network, firewall prompts and Local Network permission.

Install SpacetimeDB using the official installer documented in README.md, then pin it:

```bash
spacetime version install 2.14.1 --use
spacetime --version
spacetime start
```

In another terminal, with Rust 1.99.1 and the wasm32 target installed:

```bash
spacetime publish -s local -p crates/control pool -y
spacetime sql -s local pool "SELECT * FROM cluster_status"
```

Always pass `-s local` for calls to the database. SpacetimeDB is on main:3000.
The publisher is admin; the coordinator's separate SDK identity needs `add_admin`
per CONTRACTS.md. Manual inference below can run before control-plane wiring is finished.

## 2. Verify the model

Get the exact approved GGUF source URL and quantization; "Gemma 4 12B" alone does not
identify a file. Put it in models/ on main only. Before downloading:

```bash
git check-ignore models/example.gguf
df -h models
```

After download, set MODEL to its real path, then record:

```bash
export MODEL="/absolute/path/to/approved-model.gguf"
stat -f '%N %z bytes' "$MODEL"
shasum -a 256 "$MODEL"
```

Compare with the source's published SHA-256 when available. A locally calculated hash
alone does not establish source integrity. Fill in models.md; ask WP-5 for actual
layer count, KV bytes/token, and fit at the selected context. Do not commit model files.

## 3. Standalone baseline

Proposed controlled starting settings: 2048 context, one slot, 128 output tokens,
seed 42 and temperature zero. These are measurement settings, not new team defaults.
First verify fit; reduce model/context if the planned memory exceeds the measured budget.

Run in foreground; Ctrl+C stops it:

```bash
llama-server -m "$MODEL" -ngl 999 -c 2048 -np 1 --host 127.0.0.1 --port 8080 --metrics -lv 5 2>&1 | tee fixtures/llama/main-alone-load.txt
```

In a second terminal, wait for HTTP 200 (503 means loading):

```bash
curl --fail http://127.0.0.1:8080/health
curl --fail --max-time 300 http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"Explain in three sentences how pooling laptop memory can run a larger language model."}],"max_tokens":128,"temperature":0,"seed":42,"stream":false}' \
  -o fixtures/llama/main-alone-response.json
```

Use returned server timing fields or the server's eval timing log for generated tokens/s;
do not divide by total request duration and call it generation speed. Save warmup plus
three measured responses with unique filenames. Record prompt, actual generated tokens,
context, quantization, timings and memory. Stop standalone before starting pooled inference.

## 4. Helpers and pooled run

Obtain actual pool IPs from each owner. On helper A and B, respectively:

```bash
rpc-server -H <helper-pool-ip> -p 50052 -c
```

Keep helper output in a terminal (or a PTY capture). The startup banner may be block
buffered when piped to a file; absence of banner is not readiness failure.
Use --list-devices plus a TCP port check. Each rpc-server serves one llama-server;
do preflight before the model connects.

On main:

```bash
nc -vz <helper-A-ip> 50052
nc -vz <helper-B-ip> 50052
llama-server --rpc <helper-A-ip>:50052,<helper-B-ip>:50052 --list-devices
```

Save the full device listing. Start with helper A alone for a two-machine smoke test.
Then use both helpers for three physical machines:

```bash
llama-server -m "$MODEL" --rpc <helper-A-ip>:50052,<helper-B-ip>:50052 \
  -ngl 999 -sm layer -c 2048 -np 1 --host 127.0.0.1 --port 8080 \
  --metrics -lv 5 2>&1 | tee fixtures/llama/pool-load.txt
```

Repeat the health/chat request with a new response filename. Inspect load logs to prove
nonzero layers on both helpers and main; connectivity alone does not prove pooling.
For explicit -ts, use the WP-5 plan in the actual device order: RPC first, local last.
Shares are layer slots, including the output slot, totaling n_layers+1 for integer shares.
Do not invent an equal split when device capacities differ; the Dell exposes multiple devices.

Record first load duration, actual per-device buffers/placement and host memory. Stop
llama-server, leave helper caches intact, relaunch identically, and record second-load duration.
Do not compare runs with differing context, prompt, quantization or output limits.

## 5. Acceptance and handoff

Checkpoint A needs an answer with layers on three physical machines and reproduction by
another teammate using this runbook. Single-machine RPC is development-only.
Send WP-2/P1/Joseph the version/device/load logs, model identity/hash, layer placement,
memory budget, first/second load durations, and timing JSON. Commit sanitized fixtures only.
D-14 leaves mid-answer drop/recovery tests outside demo scope.

For the offline demo, disconnect router WAN while keeping the private LAN alive, then repeat
health/chat and verify every needed binary/model is already local. No model downloads live.
