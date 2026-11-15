# Installed desktop product — backlog

The loop: each round ships the top item end to end on DESKTOP-9VH92BJ, with tests, the architecture
gate, a ledger entry and a push. DESKTOP-CPH4KB2 stays out until its service is restored locally.

| # | Item | Acceptance | State |
|---|------|-----------|-------|
| 1 | Installer replaces the desktop payload safely | upgrade stops the old tray, re-applies the Users ACE on `desktop\`, leaves the legacy `SummriseShell` autostart pointing at the Electron app, and the script text is held by a test | done (round 80) |
| 2 | Upgrade path proves wait/verify/health/rollback | launcher's staged pair is verified, the old child exits first, `/api/health` gates the commit, and a deliberately mismatched sidecar is refused live on 9VH with a logged restore | done (round 81) |
| 3 | Tray menu as an entry point | open transcript, open terminal, check for updates, and the running version in the menu | done (round 82) |
| 4 | Transcript readability | click-to-expand shows the full `runs.exec` command, its exit facts and a capped output preview with credential shapes replaced by `***`; a terminal line links its session, and a tool filter narrows the list | done (round 85) |
| 5 | Release-candidate installer | a 2.0.0 setup built from this tree installs and upgrades on 9VH; panel, terminal, transcript, tray and `shell=0` verified | done (round 84) |

## Release and self-update (current goal)

| # | Item | Acceptance | State |
|---|------|-----------|-------|
| 6 | Publish a signed canary | document + sidecar + installer on the mirror; `verify-release.sh` passes; pointer names the version | done (round 86) |
| 7 | Device checks and applies an update | 9VH checks on demand, launcher stages 2.0.1, health proves it, the replaced generation is `previous` | pending |
| 8 | A broken candidate is refused | device's own record shows the refusal; the serving version stays healthy | pending |
| 9 | Update state is visible | panel/tray shows running version, last check result, and update-available/refused with reason | pending |
| 10 | Install the published candidate | 9VH installs 2.0.1 from the mirror; panel, terminal, transcript, tray, shell=0 verified | pending |

## One visual system (paused release goal set aside)

| # | Item | Acceptance | State |
|---|------|-----------|-------|
| 11 | Transcript redesign | system-UI chrome, mono only for tools/commands/ids, sticky status header, turn-grouped timeline, chips (allow/running/exit 0/refused), expandable detail card, filter, light-first theme with a persisted dark toggle | done (round 87) |
| 12 | Device panel in the same system | same tokens, summary cards, list rows with chips, light/dark | pending |
| 13 | `/desktop/` theme layer | shim injects colours/type/spacing so the legacy page matches | pending |
| 14 | Side-by-side comparison | screenshots of the three surfaces; mismatches fixed | pending |
| 15 | Visual regression suite | rows/chips/theme and panel markup tests | pending |
