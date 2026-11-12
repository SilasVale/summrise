# Windows handoff

The v1 checklist's last item. Everything that can run on a headless CI runner is already automated;
this page is what is left for a person at a Windows machine, with the exact commands and the evidence
to keep.

## What the installer workflow already proves

`.github/workflows/installer.yml` runs on a tag or on `workflow_dispatch` (version, channel, port) and:

- derives the launcher and release public keys from the seed secrets, so the build embeds the key the
  sidecar and the update path verify (`the first CI-built installer failed exactly that way`);
- builds the four exes with `+crt-static` and fails by name if one is missing;
- **signs the four exes and the setup when `WINDOWS_SIGNING_PFX` is set** (the exes first, because
  `makensis` copies them into the setup);
- signs the first-generation sidecar with the launcher key and builds `SummriseSetup-<version>.exe`;
- **installs it silently on the runner** (`/S /D=C:\SummriseCI`), asserts the `SummriseLaunch` service
  is running and `/api/health` says `ok`, then uninstalls silently and asserts the service is gone.

So a green run is the silent install/uninstall and the health check. Do not repeat them by hand; keep
the run URL.

## What only a person can do

1. **The interactive window.** `--check-window` needs a real desktop; the runner is headless and the
   shell refuses session 0 by name (`check-window: this is session 0, which has no interactive
   desktop`).
2. **The visual result.** Whether the panel actually renders in WebView2, and whether it is usable, is
   a human judgement.
3. **Signature verification**, if signing was configured.

## Step 1 — build and install

On a Windows machine with the repository and a build of the four exes (or take the setup from the
workflow artifact):

```powershell
# silent install, same switches the workflow uses
Start-Process .\SummriseSetup-0.3.994.exe -ArgumentList '/S', '/D=C:\Summrise' -Wait
Get-Service SummriseLaunch | Select-Object Status
curl.exe -s http://127.0.0.1:18080/api/health
```

Keep the `/api/health` JSON: it names the version, so it is the proof that the installed build is the
build that was published.

## Step 2 — the console is down: the window must open the device's panel

The shell's decision is `console URL` when a 900 ms TCP probe answers, `http://127.0.0.1:<port>/panel`
otherwise. Point it at nothing:

```powershell
$env:SUMMRISE_CONSOLE_URL = "http://127.0.0.1:1/"   # a port with no listener
C:\Summrise\summrise-shell.exe --check-window
```

Expect, in the shell's note output:

```
check-window: opening http://127.0.0.1:18080/panel
```

and the window should show the device's own panel (sessions, tools) rather than a WebView2 error page.
If the console had been reachable, the same line would name the console URL instead. Record:

- the exact `check-window: opening …` line;
- a screenshot of the window;
- whether the panel's sections render.

## Step 3 — the console is up: the window must open the console

```powershell
$env:SUMMRISE_CONSOLE_URL = "http://<console>:7736/"
C:\Summrise\summrise-shell.exe --check-window
```

Expect `check-window: opening http://<console>:7736/` and the console page. (A machine without a
console configured falls back to the build default and then to the panel, which is why step 2 names
the unreachable URL explicitly.)

## Step 4 — signatures, when a certificate was configured

```powershell
foreach ($exe in 'summrise-launch','summrise-device','summrise-console','summrise-shell') {
  Get-AuthenticodeSignature "C:\Summrise\$exe.exe" | Select-Object Path, Status, SignerCertificate
}
Get-AuthenticodeSignature .\SummriseSetup-0.3.994.exe | Select-Object Status, SignerCertificate
```

`Status` must be `Valid` for all five. Without the certificate secret the build prints
`no signing certificate configured` and everything is unsigned — a known, accepted state for canary.

## Step 5 — uninstall

```powershell
Start-Process 'C:\Summrise\uninstall.exe' -ArgumentList '/S' -Wait
Get-Service SummriseLaunch -ErrorAction SilentlyContinue   # must be $null
```

## Evidence to paste back

| piece | what to paste |
|---|---|
| the workflow | the run URL and that `windows-installer` is green |
| installed version | the `/api/health` JSON |
| console down | the `check-window: opening …/panel` line and a screenshot |
| console up | the `check-window: opening <console>` line |
| signatures | the five `Status` values (or "certificate not configured") |
| uninstall | `Get-Service` returning nothing |

With those rows, `docs/design/v1-verification.md`'s "what this machine cannot prove" is answered, and
v1 is done.
