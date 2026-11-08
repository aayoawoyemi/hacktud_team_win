# run-005: Checkpoint B prep, netcheck RTT from .174 (Sun 01:06 CT)

Roles: M3 Air .113 = main (SpacetimeDB, coordinator, llama-server); M5 Air .174 + M5 Air .204 = helpers. Wired gigabit (Opal + switch).

## Result: ✅ all pairs < 1 ms
`netcheck check` from m5-air-174, 20 samples each (07:06Z):

| Pair | Target | OK | p50 ms | p95 ms | max ms | ping avg ms |
|---|---|---|---|---|---|---|
| .174 → .113 | SpacetimeDB :3000 | 20/20 | 0.94 | 1.10 | 1.20 | 0.63 |
| .174 → .113 | sshd :22 | 20/20 | 0.75 | 0.92 | 1.01 | |
| .174 → .204 | rpc-server :50052 | 20/20 | 0.44 | 0.55 | 0.83 | 0.43 |
| .204 → .113 | (P5, 06:40Z) | 20/20 | 0.697 | | | |

## Context: Checkpoint B blocker at the same time
Coordinator on the M3 fails `E_RPC_UNREACHABLE` (errno 65) dialing helpers; `nc` works. P4's Network.framework probe: `localNetworkDenied` on en7.
Cause: macOS Local Network privacy. Processes launched from the Codex app (or a denied Terminal) can't dial LAN hosts.
Helpers verified fine from .174 (Python connect_ex .204:50052 = 0, firewall off). Fix: launch from Terminal.app (`.command`) per TN3179, or `ssh localhost`.

## Learned
- Probing a live rpc-server with netcheck check is harmless (port_accepting stayed true).
- `netcheck serve` left running on .174:50060 for throughput tests.
