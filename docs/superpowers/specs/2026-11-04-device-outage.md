# 2026-11-04: the device fleet went offline on a canary release

**Status:** open. Three devices stopped reporting after `0.3.272` was published to the canary channel;
the console is healthy and serves every page, but no device agent is connected. This is the record of
what happened, what is known, and how to bring them back. Written at 17:50 UTC, before any host was
reachable again.

## What happened, in order (all times UTC)

- 17:11 — `publish-canary.bash --version 0.3.272` published the release: artifact sha `ee0b95c4…`,
  the signed document, and the installer. `verify-release.sh 0.3.272` returned `ok` (pointer, digest,
  installer).
- 17:22 — I ran the device's hidden `--check-update` mode on DESKTOP-9VH92BJ (through the console's
  `runs.exec`) to see whether the channel reached it. That mode stages the release and asks the
  supervisor for the swap. Seconds later that device's channel dropped and did not return.
- ~17:27 — DESKTOP-CPH4KB2, which polls the same channel every 900 s, also dropped. It had taken the
  update without any manual action.
- 17:44 — `/fleet` showed all three offline: 9VH92BJ and CPH4KB2 down after the release,
  DESKTOP-14RJCR8 offline before it (last seen six hours earlier).
- 17:46 — the channel pointer was re-signed back to `0.3.271` (`verify-release.sh 0.3.271` → ok), so
  a recovered host cannot be broken again by the same build. The `0.3.272` artifact and document stay
  on the mirror but nothing points at them.

The console itself never restarted: `/health` uptime was continuous across the incident, and the
Audit recorded `device disconnected 502` followed by operator calls refused `503`.

## What is known

- The published pair is internally consistent: the artifact on the mirror hashes to the document's
  `sha256`, and the sidecar in `device/target/x86_64-pc-windows-msvc/release/` names the same digest.
- The same source starts and serves on Linux: `cargo build -p summrise-device` then
  `summrise-device --install … --data … --port 18737` listens and writes `device.port`, `state.json`,
  `device.lock` — so the failure is Windows-specific or supervisor-specific, not the shared code.
- `0.3.271` and `0.3.272` embed the same release and console-grant public keys (checked in the two
  binaries), so a rotated key is not the cause.
- `launch --apply` validates the staged pair **before** stopping the running app; a refused update
  cannot stop it. The stop happens after validation, inside the supervisor transaction.
- The unresolved piece: what the supervisor did after the stop. Three candidates, none provable
  without the host:
  1. the new app started but failed the two-answer `/api/health` check, and the rollback path did not
     restore the previous pair;
  2. the new app crashed on start (Windows-only path) and the supervisor's health wait timed out;
  3. the supervisor service itself stopped, and the SCM's three restarts (5 s apart) were exhausted
     before the app came back.

## What was reached from here

- Console `/mcp` for the host: `503` — its channel is down, and the console has no local-exec feature.
- `tailscale ssh` to `desktop-9vh92bj`: port 22 refuses.
- The gate MCP (`api.saisi.online`): `device not permitted by token` for this device.
- RDP (3389) and SMB (445) are open on `100.74.250.98`, but no credential for that host exists on this
  machine, and no RDP client is installed.

## Recovery (needs a local or RDP session on each host)

1. `sc query SummriseLaunch` — if `STOPPED`, `sc start SummriseLaunch`. The service starts the app from
   its baseline and waits for health; that alone should bring the device back on `0.3.271`.
2. If the service runs but the app does not answer on `127.0.0.1:18090`:
   read `C:\ProgramData\Summrise\state.json` (the swap verdict and the rollback memory), then
   `C:\SummriseInstall\baseline\current\` (the trusted pair) and `C:\SummriseInstall\*.new` (a staged
   pair that was never applied).
3. Restore the baseline pair with the launcher: `summrise-launch --restore` (from the install
   directory, elevated). Then `sc start SummriseLaunch`.
4. Verify from the console: `/fleet` shows the device online with `0.3.271`; then `terminal.list`
   through `/mcp` answers.
5. Only after all three are back: rebuild the release with a **one-machine canary** — publish, watch
   that single device return healthy, and only then let the rest see it (the watch/withdraw guard is
   `scripts/canary-watch.sh`, added with this record).

## What changes in the procedure

- **A canary release is watched, not announced.** `scripts/canary-watch.sh` polls one named device for
  a bounded time after a publish and re-signs the channel pointer back to the previous version if the
  device does not come back with the new one. Publishing and watching become one command.
- **A sidecar is copied with every artifact.** `publish-canary.bash` wrote it only into the target
  directory; the mirror now gets `summrise-device-<version>.sidecar.json` beside the exe, because the
  operator's own 0.3.271 publish had one and this one did not.
- **A device version is not published to the fleet from a tree that has not been run on Windows.**
  The canary watch is the only thing that can prove it; the Linux test suite cannot.
