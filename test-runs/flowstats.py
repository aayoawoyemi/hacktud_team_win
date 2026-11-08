#!/usr/bin/env python3
"""
CONCEPT: tiny local stats server for web/flow.html: real bytes/s on each main<->helper TCP link of llama-server.
WHY: the flow page animates each real token's path; this adds measured network traffic so the picture isn't only schematic.
DEPENDS ON: macOS `nettop`; llama-server pid. stdlib only.
AFFECTS: nothing; read-only. Serves GET /stats on :8001 with CORS.
Usage: python3 flowstats.py <llama-server pid>
"""
import json, os, pty, subprocess, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PID = sys.argv[1]
state = {"links": {}, "at": 0}


def loop():
    """Stream `nettop -L 0 -s 1` through a pty (piped, nettop buffers ~30 s; a pty makes it emit every second)
    and turn its cumulative per-connection counters into bytes/s per rpc-server link."""
    m, s = pty.openpty()
    subprocess.Popen(["nettop", "-m", "tcp", "-p", PID, "-L", "0", "-s", "1", "-J", "bytes_in,bytes_out"],
                     stdout=s, stderr=subprocess.DEVNULL)
    os.close(s)
    prev = {}
    for line in os.fdopen(m, "r"):
        if not line.startswith("tcp") or "<->" not in line:
            continue
        name, bin_, bout = (line.strip().split(",") + ["", ""])[:3]
        remote = name.split("<->")[1]
        if not remote.endswith(":50052") or not bin_ or not bout:   # rpc-server links only
            continue
        now, cur = time.time(), (int(bin_), int(bout))
        t, old = prev.get(remote, (now, cur))
        dt = max(now - t, 1e-3)
        state["links"][remote] = {"in_bps": (cur[0] - old[0]) / dt if now > t else 0, "out_bps": (cur[1] - old[1]) / dt if now > t else 0,
                                  "in_total": cur[0], "out_total": cur[1]}
        state["at"] = now
        prev[remote] = (now, cur)


class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps(state).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


threading.Thread(target=loop, daemon=True).start()
ThreadingHTTPServer(("0.0.0.0", 8001), H).serve_forever()
