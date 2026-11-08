<!--
CONCEPT: the network runbook. What the network must allow, how to set up the travel router, firewall steps per
         OS, how each machine finds its IP, and the netcheck preflight to run before every launch.
WHY: WP-10 R10.2 (chosen network, addresses, firewall steps, port plan), R10.6 (offline), R10.7 (bind rpc-server
     to the private network only). Acceptance: a teammate can add a new machine using this file alone.
DEPENDS ON: O-05 (network: travel router being bought; Dell Mobile Hotspot interim), O-17 (ports, decided Sat 7:55pm:
     50060 for netcheck), D-04 (llama.cpp RPC), D-13 (SpacetimeDB control plane on main),
     crates/netcheck (the commands below).
AFFECTS: WP-1 bring-up, WP-3 agent (rpc-server bind address), WP-13 runbook (preflight step).
Anything marked VERIFY has not been tested on the team's machines yet.
-->

# Network

How the laptops in the pool reach each other: ports, router setup, firewall rules for Windows and macOS, and a
preflight check to run before every launch.

Status (Sat 2026-11-07, 6:15pm): the network is not chosen yet (O-05). A travel router is being bought. The Dell's
Windows Mobile Hotspot is the interim network. Device-to-device traffic on CometNet is untested.

Contents:

1. [Ports](#1-ports)
2. [Travel router setup](#2-travel-router-setup)
3. [Interim: Dell Mobile Hotspot](#3-interim-dell-mobile-hotspot)
4. [Find each machine's IP](#4-find-each-machines-ip)
5. [Windows firewall](#5-windows-firewall)
6. [macOS firewall and Local Network permission](#6-macos-firewall-and-local-network-permission)
7. [Preflight with netcheck](#7-preflight-with-netcheck)
8. [Offline demo](#8-offline-demo-no-internet)
9. [Troubleshooting](#9-troubleshooting)

## 1. Ports

All TCP. "Main" is the Mac that runs llama-server and SpacetimeDB. "Helper" is every other machine (other Macs,
the Dell).

| Port | Service | Listens on | Who connects | Inbound rule needed on |
|---|---|---|---|---|
| 50052 | llama.cpp `rpc-server` | every helper | llama-server on main | every helper |
| 8080 | llama.cpp `llama-server` (HTTP API) | main | the bench runner and the demo client | main, only if a client runs on another machine |
| 3000 | SpacetimeDB (`spacetime start`) | main | every agent (helpers dial out to main) | main |
| 50060 | `netcheck serve` | any machine under test | `netcheck client` | any machine running `netcheck serve` |

- 50052 and 8080 are llama.cpp's defaults. 3000 is SpacetimeDB standalone's default (it listens on `0.0.0.0:3000`;
  source: research/spacetime/01-what-spacetime-is.md). 50060 is netcheck's port (O-17, decided).
- Helpers need no inbound port for the control plane: agents dial out to main on 3000.
- `rpc-server` binds to `127.0.0.1` by default (checked: `rpc-server --help`, build 8665). Other machines can't
  reach it until it is started with `-H`:

  ```
  rpc-server -H <this machine's IP on the pool network> -p 50052
  ```

  Bind to the pool network's address, never `0.0.0.0` on a shared network like CometNet. llama.cpp's README says
  rpc-server must never be exposed on an open network (R10.7, R-09).

## 2. Travel router setup

The router's model is not known yet. Steps below use names from GL.iNet's admin page (a common travel router).
VERIFY the menu names on the actual router.

1. Power the router. Connect one laptop to its default Wi-Fi (name and password are on the label).
2. Open the admin page. GL.iNet routers use `http://192.168.8.1`. Other brands: the address printed on the label,
   or the "Default Gateway" from `ipconfig` (Windows) or `route -n get default` (macOS). Set an admin password.
3. Wireless settings:
   - Set a new network name (SSID), for example `pool-net`, and a WPA2 or WPA3 password.
   - Use the 5 GHz band for all laptops. Turn 2.4 GHz off if the laptops keep joining it.
   - Turn client isolation off. Menu names: "AP Isolation", "Client Isolation", "Wireless Isolation". With it on,
     laptops reach the router but not each other.
   - Don't use the guest network. Guest networks usually isolate clients.
4. Connect every machine to `pool-net` and forget other networks on them (or untick "Auto-join" / "Connect
   automatically" for CometNet), so no laptop drifts back to campus Wi-Fi.
5. Fix each machine's address, using either method:
   - DHCP reservation: in the router's client list (GL.iNet: "Clients"), reserve or bind each laptop's current IP.
   - Or write down each machine's IP (section 4) and recheck it before every launch. Addresses can change when a
     laptop rejoins.
6. Record the addresses in the table below and in the netcheck targets file (section 7).

| Machine | Role | IP on pool-net | rpc-server running? |
|---|---|---|---|
| main Mac | main | | n/a |
| Mac A | helper | | |
| Mac B | helper | | |
| Dell | helper | | |

On Windows, when the Dell first joins `pool-net`, answer "Yes" to "Allow your PC to be discoverable by other PCs
and devices on this network?". That makes the network Private, which the firewall rules in section 5 need.
Check and fix it with:

```powershell
Get-NetConnectionProfile                                            # NetworkCategory must be Private
Set-NetConnectionProfile -InterfaceAlias "Wi-Fi" -NetworkCategory Private   # admin PowerShell
```

On CometNet the Dell's Wi-Fi profile is Public (checked Sat 6:20pm), so Private-profile rules don't apply there.

Wired option (Preston, 7:35pm): the team is also buying a switch, and the Macs need USB-C Ethernet adapters (not bought
yet). Cables cut RTT and jitter compared with Wi-Fi, so prefer them when the adapters arrive. On a cable, plug every
machine into the router's LAN ports or the switch, not the WAN port. On Windows the wired adapter gets its own
profile, so set it to Private too (`Set-NetConnectionProfile -InterfaceAlias "Ethernet" -NetworkCategory Private`;
the alias is in `Get-NetConnectionProfile`). Record the wired IPs in the table above, because they differ from the
Wi-Fi ones.

## 3. Interim: Dell Mobile Hotspot

Until the router arrives, the Dell shares a Wi-Fi network from Windows.

1. Settings > Network & internet > Mobile hotspot. Share over: Wi-Fi. Edit the name and password. Turn it on.
2. Turn off "Power saving" on the same page, so the hotspot doesn't stop when no device is connected.
3. Macs join that network. The Dell's own address on it is usually `192.168.137.1`; clients get `192.168.137.x`.
   VERIFY with `ipconfig` on the Dell (adapter "Local Area Connection* N").
4. Windows allows 8 devices at most.

VERIFY before relying on it:

- Mac-to-Mac traffic through the Dell's hotspot (client isolation behaviour is undocumented). Test: `netcheck serve`
  on Mac A, `netcheck client <Mac A IP>:50060` on Mac B.
- The firewall profile of the hotspot adapter. If it is Public, the Private-only rules in section 5 don't cover
  Macs connecting to the Dell's rpc-server. Check with `Get-NetConnectionProfile`. If needed, add the same rules
  with `-Profile Private,Public` while on the hotspot, and remove them afterwards.

## 4. Find each machine's IP

Use the IPv4 address of the adapter connected to the pool network.

Windows (PowerShell):

```powershell
Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias "Wi-Fi" | Select IPAddress
ipconfig      # look for "Wireless LAN adapter Wi-Fi" > "IPv4 Address"
```

On the Dell as hotspot host, the pool address is on "Local Area Connection* N", not on "Wi-Fi".

macOS (Terminal):

```bash
ipconfig getifaddr en0    # Wi-Fi is en0 on MacBooks; if it prints nothing, try en1
```

Or System Settings > Wi-Fi > Details (next to the connected network) > TCP/IP > IP address.

`netcheck serve` also prints a best guess ("this machine's LAN IP is probably ..."). With several adapters (VPN,
Hyper-V, Docker) the guess can be wrong, so confirm it with the commands above.

## 5. Windows firewall

Run in an admin PowerShell (Start > type "PowerShell" > right-click > Run as administrator). The rules apply to the
Private profile only, so they are inactive on CometNet and other Public networks.

Helper (the Dell's role today):

```powershell
New-NetFirewallRule -DisplayName "pool llama rpc-server 50052" -Direction Inbound -Protocol TCP -LocalPort 50052 -Action Allow -Profile Private
New-NetFirewallRule -DisplayName "pool netcheck 50060" -Direction Inbound -Protocol TCP -LocalPort 50060 -Action Allow -Profile Private
```

Only if the Dell ever runs main's services:

```powershell
New-NetFirewallRule -DisplayName "pool llama-server 8080" -Direction Inbound -Protocol TCP -LocalPort 8080 -Action Allow -Profile Private
New-NetFirewallRule -DisplayName "pool spacetimedb 3000" -Direction Inbound -Protocol TCP -LocalPort 3000 -Action Allow -Profile Private
```

Check and remove:

```powershell
Get-NetFirewallRule -DisplayName "pool *" | Select DisplayName, Enabled, Profile, Action
Remove-NetFirewallRule -DisplayName "pool *"
```

The first time `rpc-server.exe` or `netcheck.exe` listens, Windows may show "Windows Defender Firewall has blocked
some features of this app". Tick "Private networks" and click "Allow access". If someone clicked Cancel, Windows
may have added a Block rule for that program, and Block rules beat Allow rules. Find and remove it:

```powershell
Get-NetFirewallRule -Direction Inbound -Action Block -Enabled True | Where-Object DisplayName -match "rpc-server|llama|netcheck" | Select DisplayName, Profile
Get-NetFirewallRule -Direction Inbound -Action Block -Enabled True | Where-Object DisplayName -match "rpc-server|llama|netcheck" | Remove-NetFirewallRule
```

VERIFY the exact display name Windows gives those automatic Block rules (expected: the program's file description
or name).

Quick check from another Windows machine (no netcheck needed):

```powershell
Test-NetConnection -ComputerName 192.168.8.12 -Port 50052   # TcpTestSucceeded : True
```

## 6. macOS firewall and Local Network permission

Two separate macOS controls can block the pool.

Firewall (inbound). Matters on helpers (rpc-server), on main (SpacetimeDB, llama-server if used remotely) and on any
Mac running `netcheck serve`.

1. Check it: System Settings > Network > Firewall. If it is off, inbound connections are allowed and nothing else
   is needed.
2. If it is on: the first time `rpc-server`, `llama-server`, `spacetime` or `netcheck` listens, macOS shows "Do you
   want the application "rpc-server" to accept incoming network connections?". Click **Allow**.
3. To check or change it later: System Settings > Network > Firewall > Options. Each program is listed with "Allow
   incoming connections" or "Block incoming connections". Set the pool programs to Allow. Turn off "Block all
   incoming connections" and "Enable stealth mode" for the event.
4. Same from Terminal (paths from `which rpc-server` etc.):

   ```bash
   /usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate
   sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add "$(which rpc-server)"
   sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp "$(which rpc-server)"
   /usr/libexec/ApplicationFirewall/socketfilterfw --listapps
   ```

   Repeat for `llama-server`, `spacetime` and the netcheck binary (`target/release/netcheck` in the repo).

VERIFY: unsigned binaries (a locally built netcheck, Homebrew llama.cpp) may prompt again after every rebuild or
upgrade, because the firewall tracks the binary's signature. Rerun `--add` and `--unblockapp` after rebuilding.

Local Network permission (outbound, macOS 15 Sequoia and later). Main's llama-server connects out to each helper's
rpc-server, and netcheck connects out on `check` and `client`. macOS may ask "“Terminal” would like to find and
connect to devices on your local network". Click **Allow**. The permission belongs to the app the command runs in
(Terminal, iTerm, VS Code, Ghostty). Check it at System Settings > Privacy & Security > Local Network and turn the
switch on for that app. VERIFY on the team's macOS version: when denied, connects to LAN addresses fail with
"No route to host", which netcheck reports as `unreachable`.

## 7. Preflight with netcheck

Build once per machine, from the repo root:

```bash
cargo build --release -p netcheck
```

The binary is `target/release/netcheck` (macOS) or `target\release\netcheck.exe` (Windows). Until the shared CLI
lands (O-02), `cargo run --release -p netcheck -- <command>` works too.

Commands:

- `netcheck check TARGET...`: TCP-connects to each `label=host:port` 20 times (3 s timeout each) and prints status
  and connect RTT. Works against any listener: rpc-server, llama-server, SpacetimeDB, netcheck.
- `netcheck serve`: listens on 50060 for `client`.
- `netcheck client host:50060`: against a machine running `serve`, measures echo RTT (100 round trips of 8 bytes,
  TCP_NODELAY) and pushes 64 MB each way.
- `--json` on `check` and `client` prints PRD §6.9 NetcheckResult JSON instead of the table.
- Exit code: 0 = all passed, 1 = something failed, 2 = bad arguments.

### Steps before every launch

1. On every helper, start rpc-server bound to its pool IP, and the netcheck listener:

   ```bash
   rpc-server -H <helper IP> -p 50052
   netcheck serve
   ```

2. On main, copy `crates/netcheck/targets.example.toml` to `targets.toml`, fill in the IPs, then:

   ```bash
   netcheck check --targets-file targets.toml
   ```

3. On main, once per helper, measure the link (the result goes in `docs/findings.md`, R10.5):

   ```bash
   netcheck client "mac-a=192.168.8.11:50060"
   netcheck client "mac-a=192.168.8.11:50060" --json > netcheck-mac-a.json
   ```

4. On every helper, check it can reach main's SpacetimeDB:

   ```bash
   netcheck check "main spacetimedb=192.168.8.10:3000"
   ```

5. Stop `netcheck serve` on the helpers (Ctrl+C). Leave rpc-server running for the launch.

Probing rpc-server with `check` is safe: on the Dell (build 8665), 40 back-to-back probes each logged "Accepted
client connection" / "Client connection closed" and rpc-server kept running. Run `check` before llama-server
connects, not during model load.

### What a pass looks like

`check` (example layout; numbers from a loopback run on the Dell, Wi-Fi numbers will be higher):

```
netcheck check from STARROXPS at 2026-11-08T00:21:28Z

TARGET            ADDR                STATUS     OK/N    MIN ms    P50 ms    P95 ms    MAX ms  NOTE
mac-a rpc-server  192.168.8.11:50052  ok        20/20      0.37      0.67      1.21      1.49

PASS: 1 of 1 targets reachable.
```

Pass:

- Last line starts with `PASS`, exit code 0.
- Every row has STATUS `ok` and OK/N `20/20`.
- p95 under 20 ms. Above that, the NOTE column says `slow` (a warning, not a failure). The 20 ms line is a guess
  for a healthy Wi-Fi LAN hop (usually 1 to 10 ms). VERIFY against the first real measurements.

`client` pass: the last line is `PASS`, echo RTT has 100/100 samples, and both send and recv print MB/s. Expected
on 5 GHz Wi-Fi through one router: roughly 10 to 60 MB/s each way (VERIFY). Model-load time per helper is about
the helper's share of the GGUF divided by recv MB/s on main's side: a 3 GB share at 30 MB/s takes about 100 s.

### Failure kinds

| STATUS | Meaning | Fix |
|---|---|---|
| `refused` | The machine answered, but nothing listens on that port. | Start the service; check the port; check rpc-server was started with `-H <IP>` and not on the default 127.0.0.1. |
| `timed_out` | No answer within 3 s. | Firewall dropping the port (sections 5, 6); wrong IP; machine asleep. |
| `unreachable` | No route to that address. | Machine on another network; client isolation on; macOS Local Network permission denied. |
| `resolve_failed` | The host name didn't resolve. | Use the IP address. |

Windows takes about 2 s to report `refused` (it retries the connection), even on 127.0.0.1. That is why the
default timeout is 3 s. With `--timeout-ms` below 2500, a refused port on Windows shows as `timed_out`.

## 8. Offline demo (no internet)

D-02: the demo runs with no internet. R10.6: show that, and keep the local network working.

- Travel router: unplug the WAN cable, or disconnect its repeater/upstream Wi-Fi in the admin page. The router keeps
  serving `pool-net` and DHCP. VERIFY on the actual router, then run section 7 again with the WAN unplugged.
- Show it: `ping 1.1.1.1` fails, `netcheck check --targets-file targets.toml` passes.
- Dell Mobile Hotspot: turning off the Dell's upstream Wi-Fi may stop the hotspot. VERIFY before using it for the
  offline demo.
- Phone hotspot: with mobile data off, some phones stop routing between devices. VERIFY before relying on it.

## 9. Troubleshooting

- Keep laptops awake and plugged in. Windows: `powercfg /change standby-timeout-ac 0`. macOS: run `caffeinate -dims`
  in a Terminal tab during the run.
- `check` passes but llama-server can't reach a helper: the helper's rpc-server may be bound to 127.0.0.1 (default).
  Restart it with `-H <helper IP>`.
- `client` fails with "peer is not netcheck": that port runs something else. Use the netcheck port (50060).
- A Mac's IP changed: rerun section 4 on it, update `targets.toml`, and add a DHCP reservation on the router.
- `serve` fails with "can't listen on 0.0.0.0:50060": another netcheck is already running there. Stop it, or use
  `netcheck serve --bind 0.0.0.0:50061`.
