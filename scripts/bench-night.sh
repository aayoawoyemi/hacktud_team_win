#!/bin/bash
# CONCEPT: unattended night bench (WP-8). Runs B1..B5 on main (the M3) one after another, each one:
#   write configs -> start llama-server (single) or the coordinator (pool) -> wait for /health -> bench -> stop.
#   Everything lands in test-runs/bench/night-<stamp>/<RUN>/ plus SUMMARY.md. A failed run is recorded and skipped.
# WHY Terminal: macOS Local Network privacy (Apple TN3179) blocks LAN connects from binaries whose responsible app
#   (Codex) has no local-network permission (errno 65). Command-line tools run from Terminal.app or over SSH,
#   including their child processes, are allowed. So start this with `open -a Terminal scripts/bench-night.command`.
# AFFECTS: test-runs/bench/ (WP-8 numbers for Devpost). Needs scripts/bench-night.env (see bench-night.env.example).
# Written for macOS bash 3.2: no associative arrays, no GNU `timeout`.
# Usage: scripts/bench-night.sh [RUN ...]    RUN = B1 B1s B2 B2s B3 B4 B5 (default: $RUNS from the env file, else all)

set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
ENV_FILE="${BENCH_NIGHT_ENV:-$HERE/bench-night.env}"
[ -f "$ENV_FILE" ] || { echo "missing $ENV_FILE (copy scripts/bench-night.env.example and fill it in)"; exit 1; }
# shellcheck disable=SC1090
. "$ENV_FILE"

: "${LLAMA_SERVER:?set in env}" "${COORDINATOR:?set in env}" "${BENCH:?set in env}" "${MODEL_12B:?set in env}"
: "${SPACETIME_TOKEN:?set in env}"
MODEL_26B="${MODEL_26B:-}"
MODEL_26B_BYTES="${MODEL_26B_BYTES:-26859861728}"
MODEL_26B_URL="${MODEL_26B_URL:-}"
SPACETIME_URI="${SPACETIME_URI:-http://192.168.8.113:3000}"
SPACETIME_DB="${SPACETIME_DB:-pool}"
PUBLIC_HOST="${PUBLIC_HOST:-192.168.8.113}"
PORT="${PORT:-8080}"
CTX="${CTX:-4096}"
MAIN_NODE="${MAIN_NODE:-m3-air-113}"
HELPER_NODES="${HELPER_NODES:-m5-air-174 p5-air-204}"
MIN_HELPERS="${MIN_HELPERS:-2}"
WARMUP="${WARMUP:-1}"
REPEATS="${REPEATS:-3}"
PROMPTS_FILE="${PROMPTS_FILE:-$REPO/prompts/bench.toml}"
SPEC_ARGS="${SPEC_ARGS:---spec-type ngram-simple}"
KV_ARGS="${KV_ARGS:--ctk q8_0 -ctv q8_0}"

if [ "$#" -gt 0 ]; then RUN_LIST="$*"; else RUN_LIST="${RUNS:-B1 B1s B2 B2s B3 B4 B5}"; fi
STAMP="$(date +%Y%m%d-%H%M)"
NIGHT="${OUT_ROOT:-$REPO/test-runs/bench}/night-$STAMP"
mkdir -p "$NIGHT"
LOG="$NIGHT/night.log"
SUMMARY="$NIGHT/SUMMARY.md"
LPID=""

port_busy() { lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >/dev/null 2>&1; }
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

# Refuse to start on top of a live server: a running coordinator would relaunch llama-server and fight this script.
if port_busy; then
  echo "ABORT: something already listens on :$PORT. Stop the running coordinator and llama-server first."; exit 1
fi

# Keep this Mac awake for as long as the script runs.
caffeinate -dimsu -w $$ >/dev/null 2>&1 &

toml_list() { # words -> ["a", "b"]
  local out="" x
  for x in "$@"; do out="$out${out:+, }\"$x\""; done
  echo "[$out]"
}

size_of() { stat -f %z "$1" 2>/dev/null || echo 0; }

http_code() { curl -s -o /dev/null -w '%{http_code}' --max-time 3 "$1" 2>/dev/null || echo 000; }


stop_all() {
  if [ -n "$LPID" ]; then kill "$LPID" 2>/dev/null; fi
  sleep 2
  # The coordinator's Drop kills llama-server, but SIGTERM skips Drop: stop the child explicitly.
  pkill -f "$LLAMA_SERVER" 2>/dev/null
  local i=0
  while port_busy && [ "$i" -lt 30 ]; do sleep 1; i=$((i + 1)); done
  if port_busy; then pkill -9 -f "$LLAMA_SERVER" 2>/dev/null; sleep 2; fi
  if [ -n "$LPID" ]; then kill -9 "$LPID" 2>/dev/null; fi
  pkill -f "sleep 864001" 2>/dev/null
  LPID=""
}
trap 'log "interrupted"; stop_all; exit 130' INT TERM

# Run table. Sets MODE (single|pool), SIZE (12|26), FA (on|""), EXTRA (llama-server args), SCEN, NOTE.
run_spec() {
  FA=""; EXTRA=""
  case "$1" in
    B1)  MODE=single; SIZE=12; SCEN=single_fit; NOTE="12B, M3 alone" ;;
    B1s) MODE=single; SIZE=12; SCEN=single_fit; EXTRA="$SPEC_ARGS"; NOTE="12B, M3 alone, ngram speculative" ;;
    B2)  MODE=pool;   SIZE=12; SCEN=pool_part1; NOTE="12B, 3-Mac pool" ;;
    B2s) MODE=pool;   SIZE=12; SCEN=pool_part1; EXTRA="$SPEC_ARGS"; NOTE="12B, pool, ngram speculative" ;;
    B3)  MODE=pool;   SIZE=26; SCEN=pool_part2; NOTE="26B Q8_0, pool" ;;
    B4)  MODE=pool;   SIZE=26; SCEN=pool_part2; FA=on; EXTRA="$KV_ARGS"; NOTE="26B, pool, flash-attn + q8 KV" ;;
    B5)  MODE=pool;   SIZE=26; SCEN=pool_part2; FA=on; EXTRA="$KV_ARGS $SPEC_ARGS"; NOTE="26B, pool, B4 + ngram speculative" ;;
    *) return 1 ;;
  esac
}

# Optional: pull the 26B from a helper while the single-Mac runs go (they don't use the LAN). Pool runs wait for it,
# so the download never competes with RPC traffic.
FETCH_PID=""
fetch_26b() {
  local dir free_kb
  dir="$(dirname "$MODEL_26B")"; mkdir -p "$dir"
  free_kb="$(df -k "$dir" | awk 'NR==2 {print $4}')"
  if [ "${free_kb:-0}" -lt $(( (MODEL_26B_BYTES / 1024) + 2 * 1024 * 1024 )) ]; then
    echo "FAIL: not enough disk in $dir (${free_kb} KB free)"; return 1
  fi
  echo "fetching $MODEL_26B_URL -> $MODEL_26B"
  curl -fL --retry 3 --speed-limit 1000000 --speed-time 120 -o "$MODEL_26B.part" "$MODEL_26B_URL" || { echo "FAIL: curl"; return 1; }
  if [ "$(size_of "$MODEL_26B.part")" != "$MODEL_26B_BYTES" ]; then echo "FAIL: size $(size_of "$MODEL_26B.part")"; return 1; fi
  mv "$MODEL_26B.part" "$MODEL_26B" && echo "OK: $MODEL_26B ($MODEL_26B_BYTES bytes)"
}
if [ -n "$MODEL_26B" ] && [ -n "$MODEL_26B_URL" ] && [ "$(size_of "$MODEL_26B")" != "$MODEL_26B_BYTES" ]; then
  fetch_26b > "$NIGHT/fetch-26b.log" 2>&1 &
  FETCH_PID=$!
  log "26B download started in the background (pid $FETCH_PID, log fetch-26b.log)"
fi

{
  echo "# Night bench $STAMP"
  echo
  echo "Main: $PUBLIC_HOST ($MAIN_NODE). Helpers: $HELPER_NODES. llama-server: $LLAMA_SERVER"
  echo "Warmup $WARMUP, repeats $REPEATS, ctx $CTX, prompts $(basename "$PROMPTS_FILE") (use = bench)."
  echo
  echo "| Run | What | Status | Load s | RPC devices in log |"
  echo "|---|---|---|---|---|"
} > "$SUMMARY"
TABLES=""

for RUN in $RUN_LIST; do
  if ! run_spec "$RUN"; then log "$RUN: unknown run, skipped"; continue; fi
  DIR="$NIGHT/$RUN"; mkdir -p "$DIR"
  STATUS=""; LOAD_S="-"; RPC_N="-"
  if [ "$SIZE" = 12 ]; then MODEL="$MODEL_12B"; MID="gemma-4-12b-it-q4_k_m"; else MODEL="$MODEL_26B"; MID="gemma-4-26b-a4b-it-q8_0"; fi

  if [ "$MODE" = pool ] && [ -n "$FETCH_PID" ]; then
    log "$RUN: waiting for the 26B download to finish so the LAN is quiet"
    wait "$FETCH_PID"; FETCH_PID=""
    log "download: $(tail -1 "$NIGHT/fetch-26b.log")"
  fi
  if [ "$SIZE" = 26 ] && [ "$(size_of "$MODEL")" != "$MODEL_26B_BYTES" ]; then
    STATUS="SKIP: 26B not on this Mac at the right size ($(size_of "$MODEL") bytes)"
  fi
  if [ -z "$STATUS" ] && port_busy; then
    log "$RUN: port $PORT busy, stopping the old server"; stop_all
    port_busy && STATUS="SKIP: port $PORT still busy"
  fi

  if [ -z "$STATUS" ]; then
    log "$RUN: start ($NOTE)"
    T0=$(date +%s)
    if [ "$MODE" = single ]; then
      FA_ARGS=""; [ -n "$FA" ] && FA_ARGS="-fa $FA"
      # shellcheck disable=SC2086
      echo "$LLAMA_SERVER -m $MODEL -ngl 99 -c $CTX --host 0.0.0.0 --port $PORT $FA_ARGS $EXTRA" > "$DIR/cmd.txt"
      # shellcheck disable=SC2086
      "$LLAMA_SERVER" -m "$MODEL" -ngl 99 -c "$CTX" --host 0.0.0.0 --port "$PORT" $FA_ARGS $EXTRA > "$DIR/llama-server.log" 2>&1 &
      LPID=$!; READY_MAX=600; NODES="$MAIN_NODE"
    else
      # shellcheck disable=SC2086
      {
        echo "spacetime_uri = \"$SPACETIME_URI\""
        echo "spacetime_db = \"$SPACETIME_DB\""
        echo "llama_server_path = \"$LLAMA_SERVER\""
        echo "model_path = \"$MODEL\""
        echo "model_id = \"$MID\""
        echo "ctx_size = $CTX"
        echo "host = \"0.0.0.0\""
        echo "port = $PORT"
        echo "log_dir = \"$DIR\""
        echo "ready_timeout_s = 1800"
        echo "public_host = \"$PUBLIC_HOST\""
        echo "min_helpers = $MIN_HELPERS"
        [ -n "$FA" ] && echo "flash_attn = \"$FA\""
        [ -n "$EXTRA" ] && echo "extra_args = $(toml_list $EXTRA)"
      } > "$DIR/coordinator.toml"
      # The coordinator stops on Enter/EOF on stdin, and bash gives background jobs stdin=/dev/null (P2, #46).
      # A never-writing pipe keeps stdin open; $! is still the coordinator's pid.
      sleep 864001 | SPACETIME_TOKEN="$SPACETIME_TOKEN" "$COORDINATOR" "$DIR/coordinator.toml" > "$DIR/coordinator.log" 2>&1 &
      LPID=$!; READY_MAX=1200; [ "$SIZE" = 26 ] && READY_MAX=2400; NODES="$HELPER_NODES $MAIN_NODE"
    fi

    # Wait for llama-server to answer /health with 200 (model loaded), or for the launcher to die.
    while :; do
      [ "$(http_code "http://127.0.0.1:$PORT/health")" = 200 ] && break
      if ! kill -0 "$LPID" 2>/dev/null; then STATUS="FAIL: launcher exited before ready (see logs)"; break; fi
      if [ $(( $(date +%s) - T0 )) -gt "$READY_MAX" ]; then STATUS="FAIL: not ready after ${READY_MAX}s"; break; fi
      sleep 5
    done
  fi

  if [ -z "$STATUS" ]; then
    LOAD_S=$(( $(date +%s) - T0 ))
    # Evidence that the pool really split the model: llama-server prints one "using device" line per device.
    grep -hiE "using device|RPC|50052|offloaded" "$DIR"/llama-server*.log 2>/dev/null | head -40 > "$DIR/devices.txt"
    RPC_N=$(grep -ciE "using device RPC" "$DIR/devices.txt")
    log "$RUN: ready in ${LOAD_S}s, RPC device lines: $RPC_N"
    {
      echo "endpoint = \"http://127.0.0.1:$PORT\""
      echo "prompts_file = \"$PROMPTS_FILE\""
      echo "output_dir = \"$DIR/bench-out\""
      echo "scenario = \"$SCEN\""
      echo "model_id = \"$MID\""
      echo "plan_id = \"$RUN\""
      # shellcheck disable=SC2086
      echo "nodes = $(toml_list $NODES)"
      echo "ctx_size = $CTX"
      [ "$SIZE" = 26 ] && echo "size_ratio = 1.4"
      echo "prompt_use = \"bench\""
      echo "warmup = $WARMUP"
      echo "repeats = $REPEATS"
      echo "timeout_s = 600"
    } > "$DIR/bench.toml"
    if "$BENCH" "$DIR/bench.toml" > "$DIR/bench.log" 2>&1; then
      STATUS="OK"
      [ "$MODE" = pool ] && [ "$RPC_N" = 0 ] && STATUS="OK (check devices.txt: no RPC device line matched)"
    else
      STATUS="FAIL: bench exited non-zero (bench.log)"
    fi
    MD="$(ls "$DIR"/bench-out/*.md 2>/dev/null | head -1)"
    [ -n "$MD" ] && TABLES="$TABLES
## $RUN: $NOTE

Flags: ${FA:+-fa $FA }${EXTRA:-none}. Load ${LOAD_S}s.

$(cat "$MD")
"
  fi

  log "$RUN: $STATUS"
  echo "| $RUN | $NOTE | $STATUS | $LOAD_S | $RPC_N |" >> "$SUMMARY"
  stop_all
done

echo "$TABLES" >> "$SUMMARY"
log "done. Summary: $SUMMARY"
