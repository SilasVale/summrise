# Installed desktop product — backlog

The loop: each round ships the top item end to end on DESKTOP-9VH92BJ, with tests, the architecture
gate, a ledger entry and a push. DESKTOP-CPH4KB2 stays out until its service is restored locally.

| # | Item | Acceptance | State |
|---|------|-----------|-------|
| 1 | Installer replaces the desktop payload safely | upgrade stops the old tray, re-applies the Users ACE on `desktop\`, leaves the legacy `SummriseShell` autostart pointing at the Electron app, and the script text is held by a test | done (round 80) |
| 2 | Upgrade path proves wait/verify/health/rollback | launcher's staged pair is verified, the old child exits first, `/api/health` gates the commit, and a deliberately mismatched sidecar is refused live on 9VH with a logged restore | pending |
| 3 | Tray menu as an entry point | open transcript, open terminal, check for updates, and the running version in the menu | pending |
| 4 | Transcript readability | click-to-expand shows the full `runs.exec` command and its output, a terminal line links its session, and a tool filter narrows the list | pending |
| 5 | Release-candidate installer | a 2.0.0 setup built from this tree installs and upgrades on 9VH; panel, terminal, transcript, tray and `shell=0` verified | pending |
