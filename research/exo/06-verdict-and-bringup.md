# 06 Verdict and bring-up (EXO for 3 MacBooks + Nemotron)

Pinned code: exo main `a60b95698ca81737d367443674dd5563cbc9559b` (2026-11-06). Hardware: MacBook M5 24 GB (Aayo,
coordinator), MacBook M3 16 GB, MacBook M5 24 GB. Travel router. Defaults: Nemotron-3-Nano-30B-A3B **5-bit** live,
6-bit if memory allows; tool calls only through the pooled 30B; thinking on for the plan step, off for tool turns.

## Fast answers (asked 2026-11-07)

### Q1. Does `uv sync --extra mlx` on main compile MLX and need full Xcode?

- **Yes.** `pyproject.toml` `[tool.uv.sources]` (lines 83 to 85) pins
  `mlx = { git = "https://github.com/ml-explore/mlx.git", rev = "36969c09...", marker = "sys_platform == 'darwin'" }`.
  A git source has no wheel, so uv builds it with CMake (`[tool.uv.extra-build-dependencies] mlx = [setuptools,
  nanobind, pybind11, cmake, ninja, ...]`, line 191). MLX's Metal kernels need the `metal` compiler, which comes
  with **full Xcode plus its Metal Toolchain component**, not with the Command Line Tools.
  - The README says so: "Xcode (provides the Metal ToolChain required for MLX compilation)".
  - The Nix route exists "to avoid the Xcode Metal ToolChain" via a Cachix binary cache.
- **Size and time (estimates, not measured):**
  - Xcode: about 3 to 4 GB download and 10+ GB installed, from the App Store or a developer.apple.com xip. On
    Xcode 26 the Metal Toolchain is a separate component of about 0.7 GB
    (`xcodebuild -downloadComponent MetalToolchain`).
  - MLX build: roughly 10 to 20 min on a base M5, as it compiles hundreds of Metal kernels and C++ files.
  - Total with Xcode download: 45 to 90 min per Mac on decent internet.
- **Way to avoid Xcode, and it is safe:** that rev **is MLX v0.32.4**. The GitHub compare `v0.32.4...36969c09`
  returns `identical` (ahead 0, behind 0), with commit date 2026-10-14 ("[BUG] Metal fence deadlock (#4689)").
  - PyPI has `mlx 0.32.4` wheels for cp313 macOS 14/15/26 arm64 (0.7 MB), plus `mlx-metal 0.32.4`
    (45 MB, or 78 MB for macOS 26), uploaded 2026-10-14.
  - Deleting the 3-line darwin git source makes uv install the identical code as a wheel. No Xcode is needed:
    the Command Line Tools are enough for git and for linking the Rust `exo_rs` extension.
  - The other git sources (`mlx-lm`, `mflux`) are pure Python and don't need Xcode.
- **One rule: all 3 Macs must do the same.** Node joining compares `build_id()` =
  `"exo <ver>, mlx <installed version>, mlx-lm <ver>"` (`src/exo/routing/wire.py` line 80). A wheel install and a
  git build may report different MLX version strings and refuse to join each other.
- The exact patch (tested on the repo copy; it removes only the source block, not the `mlx` extra) is in step T2
  below.

### Q2. Does the app v1.0.71 crash on a 3-way Nemotron-3-Nano pipeline? Can a 2-way split run tonight?

- **v1.0.71 code:** tag commit `fd707de`, 2026-04-23. It has Nemotron-3-Nano cards (MXFP4/4/5/6/8-bit, BF16,
  NVFP4) and Nano-9B-v2. It has no Lightning card and no `nemotron_h/` folder; Nemotron pipeline handling is inline
  in `src/exo/worker/engines/mlx/auto_parallel.py` lines 244 to 382 (`_patch_hybrid_cache`, `fa_idx`/`ssm_idx`).
  Networking is **libp2p**, not Zenoh (`Cargo.toml` line 45). MLX is pinned to a git fork
  (`rltakashige/mlx-jaccl-fix-small-recv`). The app bundles a prebuilt MLX, so app users need no Xcode.
- **The #2515 crash needs a shard with no attention layer.**
  - Attention layers sit at block indices 5, 12, 19, 26, 33 and 42 (pattern `MEMEM*EMEMEM*...`). Any contiguous
    shard of 10 or more layers contains at least one.
  - The only attention-free stretches are 6 blocks between attention layers, and the 9-block tail 43 to 51.
  - Our splits all include attention: 3-way 20/12/20 = [0,20) [20,32) [32,52), and 2-way 26/26.
  - Also, v1.0.71 restores SSM snapshots with `deepcopy(snapshots[-2])` (`generator/generate.py` line 373), which
    keeps the patched `make_mask` on the copy. Per #2515, the crash came from main restoring into *new*
    `ArraysCache` objects.
  - **So the crash should not trigger on v1.0.71 for 2-way or 20/12/20 3-way.** This is from reading the code, not
    a test.
- **What v1.0.71 lacks:**
  - the float32 router/Mamba fixes (slightly lower fidelity vs NVIDIA reference; #2515 measured mlx-lm's bf16 path
    off by KL 1e-2);
  - the Lightning cards;
  - serving without the master (#2597);
  - Zenoh networking.
  Tensor parallel on quantized Nemotron gives garbage on v1.0.71, but we don't use tensor parallel.
- **Tonight's networking test on the app: yes, a 2-way (or 3-way) Nemotron-3-Nano 5-bit pipeline should work.**
  It validates the router, LAN, model download and memory. It does **not** validate main's Zenoh discovery
  (multicast `ff12::e0a1:de89` UDP 52413). The app's libp2p uses mDNS (UDP 5353) instead. Every Mac must run the
  same app 1.0.71 (the default for the DMG/brew today).
- **Decision:** test with the app tonight if that's what is installed. Move to the main source build (no-Xcode
  patch) for the demo only if it installs cleanly on all 3 Macs tonight. Otherwise demo on the app v1.0.71 with
  Nemotron-3-Nano 5-bit. Both work for our splits; main adds fidelity, Lightning, and master-free serving.

## One-page verdict

### What EXO is

EXO 1.0 is an open-source Python + Rust system that pools Apple Silicon Macs on a LAN into one inference cluster.
- **Cluster:** devices find each other by IPv6 multicast, elect a master, and share an event-sourced cluster state
  over Zenoh.
- **Placement:** the master places a model across the fewest Macs that have enough free memory, splitting layers
  in proportion to each Mac's free memory (pipeline) or evenly (tensor, RDMA only).
- **Inference:** MLX on Metal. Any node serves an OpenAI/Claude/Ollama-compatible API on port 52415 and routes each
  request to the least-busy instance of the requested model.
- **Scope:** Macs only (no Windows, and Mac+Linux mixes are refused). No node auth or encryption.

### Reuse directly

- exo main as the pooling engine, unmodified except the no-Xcode MLX source patch.
- Its HTTP API as our backend contract:
  - `/v1/chat/completions` with `tools` and `enable_thinking`;
  - `/instance/previews`, `/place_instance`, `/instance/await`, `DELETE /instance/{id}`;
  - `/state`, `/membership`, `/node_id`, `/download/start`.
- Its built-in least-in-flight routing across instances.
- Its built-in Nemotron cards and sharding (`models/nemotron_h/`), and its qwen3_coder tool parser for Nemotron.
- Its dashboard, as a backup view.

### Copy as patterns (in our Spacetime control plane or UI)

- **Placement preview before launch:** `/instance/previews` returns per-node memory deltas. Show "what each laptop
  will hold" before loading.
- **Layer map:** `GET /state` → `instances[*].shardAssignments` (layer ranges per node) drives our "who runs which
  layers" view. Keys are camelCase in state JSON (`FrozenModel` uses `to_camel`); check the real keys on day 1.
- **Contribution knob:** `OVERRIDE_MEMORY_MB` per Mac = how much memory it lends.
- **Deployment with constraints** (`min_nodes`, `pinned_nodes`) that the master re-places when a node drops.
- **Version gate:** the cluster refuses mismatched builds. Our join code should check the exo commit and MLX version
  too.

### Avoid

- The app v1.0.71 for the final demo if main works (it is older: libp2p, no Nemotron fixes).
- Tensor parallel (needs TB5 RDMA; has had Nemotron quantized bugs).
- bf16 models on a 3-node pipeline (#2196: Nemotron-3-Nano bf16 took 111 s to first token).
- Pulling a pool Mac live (the request fails; with `min_nodes=3` the model waits for that Mac).
- The 9B model for tool calls (no parser in EXO).
- Relying on `tool_choice` (ignored).
- Venue Wi-Fi (multicast, client isolation).

### Where to start

Get the 3-Mac Nemotron 5-bit pipeline answering one tool-call request on the travel router. Then wire the Spacetime
control plane to the exo API: devices from `/state`, jobs to `/v1/chat/completions`, agent steps written as they
stream.

## Bring-up, copy-paste (3 Macs)

Roles: **COORD** = Aayo's M5 (runs `--force-master`, our backend, the dashboard). **M3** = the 16 GB Mac.
**M5B** = the other M5. Model folder on macOS: `~/.exo/models/`.

### Tonight T0: decide app or source (5 min)

If the team is already on the app 1.0.71 and wants only a networking test, skip to T5 (app variant). For the demo
build, do T1 to T4 on **all 3 Macs**, identically.

### T1. Toolchain (each Mac, about 10 to 20 min, needs internet)

```bash
xcode-select --install          # Command Line Tools only; full Xcode NOT needed with the T2 patch
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
eval "$(/opt/homebrew/bin/brew shellenv)"
brew install uv node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

### T2. exo at the pinned commit, with MLX from PyPI (each Mac, about 10 to 15 min)

```bash
git clone https://github.com/exo-explore/exo ~/exo
cd ~/exo
git checkout a60b95698ca81737d367443674dd5563cbc9559b
# Use the PyPI MLX 0.32.4 wheel (identical to the pinned git rev 36969c09) instead of compiling MLX with Xcode
python3 - <<'EOF'
import re, pathlib
p = pathlib.Path("pyproject.toml"); s = p.read_text()
s2 = re.sub(r'\nmlx = \[\n  \{ git = "https://github.com/ml-explore/mlx\.git"[^\n]*\n\]\n', '\n', s)
assert s2 != s, "patch did not apply"
p.write_text(s2); print("patched")
EOF
cd dashboard && npm install && npm run build && cd ..
uv sync --extra mlx
uv run python -c "import mlx.core as mx; print('mlx', mx.__version__, 'metal', mx.metal.is_available())"
uv run exo --version
```

Expected: `mlx 0.32.4 metal True`. **All 3 Macs must print the same `exo` and `mlx` versions.**
macmon is optional; without it the dashboard shows memory but not GPU % or temperature. To install it:
`cargo install --git https://github.com/vladkens/macmon --rev a1cd06b6cc0d5e61db24fd8832e74cd992097a7d macmon --force`.

Quit the EXO app first if it is installed (it uses the same ports 52413 to 52415).

### T3. Download the model once (COORD, home internet, about 22 GB for 5-bit, +26 GB for 6-bit)

```bash
cd ~/exo && uv run exo --force-master &          # wait ~10 s
API=http://localhost:52415
NODE=$(curl -s $API/node_id | tr -d '"')
for M in mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-5Bit ; do   # add -6Bit only if disk allows
  curl -s -X POST $API/download/start -H 'content-type: application/json' \
    -d "{\"target_node_id\":\"$NODE\",\"model_id\":\"$M\"}"; echo
done
# optional routing model (5 GB): mlx-community/NVIDIA-Nemotron-Nano-9B-v2-4bits
watch -n 10 'du -sh ~/.exo/models/*'
ls ~/.exo/models/mlx-community--NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-5Bit/.exo-download.json && echo complete-manifest
```

(Or click Download in the dashboard at http://localhost:52415.)

### T4. Copy the model to M3 and M5B over the router (each target Mac, about 3 min on 1 GbE, 5 to 15 min on Wi-Fi)

On COORD: System Settings → General → Sharing → **Remote Login ON**. Note COORD's username and `hostname`.

```bash
# on M3 and on M5B (exo must not be running)
mkdir -p ~/.exo/models
rsync -a --progress <coord-user>@<coord-hostname>.local:.exo/models/mlx-community--NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-5Bit ~/.exo/models/
```

### T5. Start the cluster offline (venue, on the travel router)

On every Mac: join the travel router (Ethernet if there are adapters), quit heavy apps, then:

```bash
# GPU wired limit (resets on reboot). 24 GB Macs:
sudo sysctl iogpu.wired_limit_mb=19456
# 16 GB Mac (M3):
sudo sysctl iogpu.wired_limit_mb=11264
cd ~/exo
# COORD:
EXO_OFFLINE=true uv run exo --namespace hacktud-pool --force-master
# M3 and M5B:
EXO_OFFLINE=true uv run exo --namespace hacktud-pool
```

Click **Allow** on the macOS "Local Network" prompt on every Mac.

App variant (v1.0.71): Settings › Cluster › Cluster name `hacktud-pool`, Settings › Offline ON, on all 3 Macs.

Check from COORD:

```bash
API=http://localhost:52415
curl -s $API/membership | python3 -m json.tool | head -40      # 3 members
curl -s $API/state | python3 -c "import json,sys; s=json.load(sys.stdin); print(list(s.keys()))"
curl -s $API/state/topology | python3 -m json.tool | head -40
```

If a Mac is missing: confirm the same `uv run exo --version`, Local Network allowed, same router, and
`tail -50 ~/.exo/exo_log/exo.log`.

### T6. Place the pooled model across all 3 (COORD)

```bash
M=mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-5Bit
curl -s "$API/instance/previews?model_id=$M" | python3 -m json.tool | head -60
curl -s -X POST $API/place_instance -H 'content-type: application/json' \
  -d "{\"model_id\":\"$M\",\"sharding\":\"Pipeline\",\"instance_meta\":\"MlxRing\",\"min_nodes\":3}"; echo
curl -N "$API/instance/await?model_id=$M&timeout_seconds=300"
```

### T7. Smoke tests (COORD): chat, tool call, thinking toggle, speed, correctness

```bash
# 1. plain chat, thinking off, measure speed (usage + wall clock)
time curl -s $API/v1/chat/completions -H 'content-type: application/json' -d "{
 \"model\":\"$M\",\"enable_thinking\":false,\"temperature\":0,\"max_tokens\":200,
 \"messages\":[{\"role\":\"user\",\"content\":\"Explain what a COBOL PERFORM VARYING loop does in 3 sentences.\"}]}" | python3 -m json.tool

# 2. one tool call (expect finish_reason "tool_calls")
curl -s $API/v1/chat/completions -H 'content-type: application/json' -d "{
 \"model\":\"$M\",\"enable_thinking\":false,\"temperature\":0.2,
 \"messages\":[{\"role\":\"user\",\"content\":\"Read the file legacy/PAYROLL.cbl\"}],
 \"tools\":[{\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"description\":\"Read a file from the repo\",
   \"parameters\":{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}},\"required\":[\"path\"]}}}]}" | python3 -m json.tool

# 3. thinking on (plan step): reasoning_content should appear
curl -s $API/v1/chat/completions -H 'content-type: application/json' -d "{
 \"model\":\"$M\",\"enable_thinking\":true,\"max_tokens\":600,
 \"messages\":[{\"role\":\"user\",\"content\":\"Plan the steps to port a COBOL payroll batch job to Python.\"}]}" | python3 -m json.tool | head -40
```

Correctness check for PR #2264, the hybrid-pipeline risk: run test 1 at `temperature 0` on 3 Macs, then
`DELETE /instance/<id>` and re-place with `"min_nodes":2` (the two M5s; 5-bit = 21.7 GB fits 2 × 16 to 19 GB). Run
the same prompt again. The two answers should match token for token, or nearly. If the 3-way answer is garbled,
use 2-way for the demo.

### T8. Routing model, optional (after the pool is loaded and only if both M5s show more than 6 GB free)

```bash
R=mlx-community/NVIDIA-Nemotron-Nano-9B-v2-4bits
curl -s -X POST $API/place_instance -H 'content-type: application/json' -d "{\"model_id\":\"$R\",\"min_nodes\":1}"; echo
curl -s -X POST $API/place_instance -H 'content-type: application/json' -d "{\"model_id\":\"$R\",\"min_nodes\":1}"; echo
```

Use it for non-tool work only (summaries, chat). EXO has no parser for its `<TOOLCALL>` format.

### T9. 6-bit upgrade, only if T7 passes and memory allows (apps quit, M3 showing at least 7.5 GB usable)

`DELETE /instance/<5-bit id>`, then repeat T6 with `...-MLX-6Bit`, which needs the 6-bit files copied in T4.
Expected per-Mac memory: about 9.1 / 6.5 / 10.1 GB.

## API calls our Spacetime control plane makes (contract with worker `spacetime`)

| Purpose | Call | Notes |
|---|---|---|
| Device list and memory | `GET /state` (poll every 1 to 2 s) or `GET /events` | per node: memory (`ramAvailable`, `gpuWorkingSet`), network, topology; check the exact keys on day 1 |
| Who is in the pool | `GET /membership` | alive members |
| Layer map | `GET /state` → instances → shard assignments | `startLayer`/`endLayer` per runner, `nodeToRunner` |
| Preview a pool | `GET /instance/previews?model_id=` | memory delta per node |
| Launch the pool | `POST /place_instance` `{model_id, sharding:"Pipeline", instance_meta:"MlxRing", min_nodes:N}` | acknowledged when in state; 503 after ~45 s |
| Wait for ready | `GET /instance/await?model_id=&timeout_seconds=300` | SSE: `ready` or `timeout` |
| Rebalance (Mac joined) | `DELETE /instance/{id}` then `POST /place_instance` with `min_nodes`=count | EXO does not rebalance on join |
| Agent step | `POST /v1/chat/completions` `{model, messages, tools, enable_thinking, stream:true}` | stream; write each delta/tool call to Spacetime |
| Cancel | close the stream, or `POST /v1/cancel/{command_id}` | |
| Contribution slider | restart that Mac's exo with `OVERRIDE_MEMORY_MB=<n>` | a join.sh variable |
