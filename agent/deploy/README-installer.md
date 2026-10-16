# Summrise Windows installer — test checklist

Installer: `https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe`
(version-pinned; currently 1.2.307. Each version also has `SummriseAgent-Setup-<ver>.exe`.)

Self-contained (ADR 0009, since 1.2.307): the pinned tgz is packed into the installer, so
**it installs with no network** (the ~100MB Electron still has to be downloaded, and so does
the portable Node when it is missing); the embedded package deletes itself after the install.
The checks: pull the network cable on a fresh machine and run the installer → the agent itself
and the panel work (Electron reporting "未装上" ("not installed") is an expected WARNING), and
no `summrise-agent-*.tgz` is left behind under `scripts\`.

**Test only on the spare Windows sandbox machine — never on the production device (d1)**:
an install stops the agent, reinstalls the same version and restarts the desktop shell, which
interrupts a running AI session.

## The SmartScreen blue warning (expected; it stays until there is a certificate)

The package carries no Authenticode signature, so its first run always pops
"已阻止无法识别的应用" ("Windows protected your PC"). Click **更多信息** (**More info**) →
**仍要运行** (**Run anyway**). If you do not trust it, compare hashes:
`certutil -hashfile SummriseAgent-Setup.exe SHA256` against `installer_sha256` in
`https://agent.saisi.online/summrise-agent/version.json` — a match means it was not tampered
with. The signing pipeline is ready (`sign_exe` in `build-installer.sh`: the whole self-signed
chain was verified end to end); all that is missing is a public CA certificate (OV ~$100–300/year,
requires a registered company, accrues reputation; EV ~$300–500/year, instant trust) — buy one
and set `SUMMRISE_SIGN_CRT`/`SUMMRISE_SIGN_KEY` (+ optional
`SUMMRISE_SIGN_PASS`/`SUMMRISE_SIGN_TSA`) and it works; with no certificate the build skips
signing automatically.

## Install test (a fresh machine is best)

1. Right-click → Run as administrator `SummriseAgent-Setup.exe`.
2. Wizard pages: welcome (brand art) → directory (default `C:\Program Files\Summrise`) →
   install progress → finish page.
3. The progress page should show, in order: Node (reuse an existing install or download the
   portable build) → `npm i -g` → `summrise setup` → Electron (~100MB) → tasks/shortcuts → DONE.
4. When it finishes, check:
   - `summrise --version` works and the panel opens at `http://127.0.0.1:18080/desktop/`
   - the scheduled tasks `SummriseAgent` (SYSTEM) + `SummriseDesktop` (logon) exist
   - the registry key `HKLM\SOFTWARE\Summrise\Agent\InstallDir`
   - the install root holds only `summrise-agent.exe` + `uninstall.exe`; `etc\`
     (config/marker), `components\` (node/npm-global/cloudflared/playwright/desktop shell)
     and `scripts\` are all present; logs are under `%ProgramData%\Summrise\logs\` (layout v2,
     see ADR 0008)
   - the desktop `Summrise.lnk` icon is the sunrise mark (not blank)
   - the tray + taskbar window icons are normal, and a directory analogous to `D:\Summrise`
     raises no error
   - the four-line `install-result.txt` receipt carries no WARNING (Electron not installed
     counts as a WARNING and is acceptable: the agent itself is unaffected)
5. Register the device (the installer no longer asks for a registration key): after the install,
   fill in the gateway address + registration key in the device panel's 设置 → Gateway card
   (设置 = Settings; the console path is 设备管理 → 生成注册码 = Devices → Generate key), which
   registers it in one step; or, before reinstalling, run `summrise setup --reg-key <key>` by hand.

## Integrity verification on the CDN fallback (round-124)

When the bundled tgz is missing (quarantined by antivirus, incomplete extraction) the installer
falls back to the CDN download — and that path now **verifies before it installs**: download to a
temp file → read `sha256` from `/api/version` (and the version number must match the version being
installed) → compare with `Get-FileHash` → only then hand it to npm; a mismatch, or a manifest with
no digest for that version, exits (**exit 7**) and prints the manifest address. Measured 2026-09-14:
`summrise-agent-1.2.364.tgz`, the manifest and the `latest` alias all agree on one digest
(`9ed7063e…`), so this gate passes on the real artifact.

Manual verification (needs a Windows machine, and a **deliberately** corrupted download): point
`-CdnBase` at a directory you built, put a tgz with modified contents there plus an `api/version`
listing the correct sha256, and expect **exit 7** with no npm install. What can be run inside this
repository is only the logic unit test
(`pwsh -File agent/deploy/lib/SummriseIntegrity.tests.ps1`; the CI ubuntu runner ships pwsh) and the
wiring pin (`agent/tests/installer_integrity.rs`).


## Uninstall test

Control Panel → Uninstall Summrise Agent: the tasks/program directory/registry/shortcuts should be
cleared, and `%ProgramData%\Summrise` data is kept by default (only
`summrise uninstall --purge-data` deletes it).

## What to report back

Version + machine (fresh/overwrite) + OK/FAIL per item in sections 4/5 above + the relevant excerpt
from `installer.log` (under `%ProgramData%\Summrise\logs\`).
