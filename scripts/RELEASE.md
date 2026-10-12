# Release — npm is the only channel

**A RUNBOOK, KEPT OUT OF `AGENTS.md` ON PURPOSE: it is read when you release, not every session.** The rules that
must hold at all times stay in `AGENTS.md` (the tag's rules, the push discipline).

```bash
# 1. bump agent/summrise-agent-npm/package.json "version" to 1.2.N, then:
touch agent/src/lib.rs && ./scripts/build.sh agent
cp agent/target/x86_64-pc-windows-msvc/release/summrise-{agent,launch}.exe agent/summrise-agent-npm/
mkdir -p agent/summrise-agent-npm/bin
cp agent/target/x86_64-pc-windows-msvc/release/summrise-cli.exe agent/summrise-agent-npm/bin/summrise.exe
#    THREE EXES, AND THE THIRD IS THE CLI. `bin/summrise.exe` is the npm `bin` (landing 4b) — the
#    command a device runs — and its version is COMPILED IN from package.json, so the bump above is
#    only in the binary if the build came after it. publish-release.sh dates the staged copy against
#    that manifest and refuses one older than it, which is the deadlock guard: a CLI older than the
#    release it manages tells the operator to install something npm cannot deliver.
#    The launcher is what SummriseDesktop and SummrisePlaywright run instead of the retired
#    .vbs wrappers, and it is COPIED on the device, never built there — a release without it ships tasks
#    that cannot start. publish-release.sh refuses a missing or stale staged copy and required-in-tgz.txt
#    refuses a tarball without any of the three, both BEFORE the upload.
#    ./scripts/publish-release.sh 1.2.N --dry-run — every gate, no credentials, changes nothing, seconds.
# 2. publish to BOTH channels (pack + manifest + prune + deploy + smoke; it does NOT commit):
./scripts/publish-release.sh 1.2.N --npm      # needs $NPM_TOKEN or ~/.npm-token
# 3. the release commit goes on a BRANCH and reaches main by merge, like everything else:
git checkout -b release/1.2.N
git add agent/summrise-agent-npm/package.json index/public/summrise-agent/version.json
git commit -m "release: 1.2.N"
git checkout main && git merge --no-ff release/1.2.N && git push origin main
# 4. WAIT for that commit's CI to go GREEN, then tag it through the API (pushing tags times out here):
curl -sX POST -H "Authorization: Bearer $(cat ~/.github-token)" \
  https://api.github.com/repos/SilasVale/summrise/git/refs \
  -d "{\"ref\":\"refs/tags/v1.2.N\",\"sha\":\"$(git rev-parse HEAD)\"}"
# 5. audit CDN vs the GitHub asset, byte for byte (it needs the asset, which is why step 4 comes first):
./scripts/publish-release.sh --audit-only 1.2.N
```

**THE TAG'S RULES, EACH ONE PAID FOR.** The SHA must be a PUSHED commit whose CI is GREEN — a local commit answers
"Object does not exist", and a superseded one fails `release.yml`'s own gate. The tag must NOT MOVE onto different
content: a move demotes the release to a DRAFT (invisible to the audit's `GET /releases/tags/<tag>`) and makes CI package
a different artifact under the same version number. **Do not push anything while a release commit's CI is running** — a
push cancels it, and the tag then has no green CI to point at. If CI must go green again for an already-published
version, put an EMPTY commit on the release commit: the tree is unchanged, so the asset matches what shipped.

**AND `ci.yml` TRIGGERS ON `main` ONLY, WHICH MAKES THE EMPTY COMMIT A TWO-STEP.** `release.yml`'s gate is FAIL-CLOSED
on silence ("zero check-runs + zero statuses means CI has not reported yet, or never will — WAIT, never pass"), so a
commit no workflow has ever seen can never be tagged, however green the tree is. The empty commit must be PUSHED first —
a dispatch names a ref the runner has to fetch — and then dispatched:

    gh api repos/$REPO/actions/workflows/ci.yml/dispatches -f ref=<branch>      # HTTP 204

**PUBLISH TO `latest`, NOT `alpha`.** The CDN's `-latest.tgz` alias moves on every release, so npm's `latest` must move
with it; a CLI older than the release it manages deadlocks the device, because the CLI's own guard tells the operator to
install a version `npm i -g` then cannot deliver. `--npm-tag alpha` remains for a deliberate prerelease channel.

**ON THE DEVICE (PowerShell) — AND THE `--prefix` MATTERS.** Without it npm installs elsewhere, reports success, and
`summrise update` ships the old exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command summrise).Source) https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise update
summrise status
```

**THAT SUBSTITUTION FAILS SILENTLY ANYWHERE BUT POWERSHELL.** Run from a `cmd`-style shell — which is what a device
tool's terminal often is — `(Split-Path …)` is not evaluated: npm reads it as a PACKAGE NAME and fails with
`E404 … '(Get-Command@*'`. The obvious repair is worse: the `install dir:` that `summrise status` prints is NOT the npm
prefix, which is the directory the `summrise` command resolves from, so npm installs into the WRONG place and still
reports `added 1 package`. The form that needs no quoting, because the 8.3 short name has no spaces:

    (Get-Command summrise.cmd).Source                # -> D:\Program Files\nodejs\summrise.cmd
    #   NOT `where summrise` — it prints NOTHING in the agent-hosted PTY, which runs as SYSTEM with a cwd
    #   of C:\Windows\System32\config\systemprofile and a PATH holding no npm prefix. summrise IS installed.
    npm i -g --prefix D:\PROGRA~1\nodejs https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
    summrise status   # `this CLI:` is the ONLY proof; npm prints "changed 1 package" either way

**A BARE `npm i -g summrise-agent` CAN INSTALL NOTHING WHILE REPORTING SUCCESS.** A stale `latest` resolved from npm's
cache or this box's mirror printed `changed 1 package` and left the OLD version in place. The URL form above has no
resolution step and is immune; an EXACT version is what `setup` uses for the same reason. Verify with `summrise status`,
never npm's exit code.

**`summrise update` ANSWERS 502 WHILE IT SWAPS THE EXE — that is the swap, not a failure**, and the dark window runs from
minutes to HOURS: measured, a tunnel answering 502 then 530/1033 for ~2 hours while `startup.log` showed the agent
starting normally with its new release marker. The retries total ~25 seconds, so the window is the tunnel with no
connector. **Read `startup.log` before calling a device broken.**

**THREE THINGS BITE A FIRST-TIME WINDOWS INSTALL:**

  1. **PowerShell REFUSES `summrise`** — it prefers npm's `.ps1` shim, which the default `Restricted` policy blocks
     (*"在此系统上禁止运行脚本"*). **Use `summrise.cmd <command>`**, which bypasses the policy and changes nothing on the
     machine (or `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, once).
  2. **`setup` NEEDS AN ELEVATED SHELL AND SAYS SO BADLY** — the install dir defaults to `C:\Program Files\Summrise` and
     the registry key is `HKLM`, so a normal user's `setup` dies with `EPERM: … mkdir 'C:\Program Files\Summrise\scripts'`,
     a stack trace where the useful sentence is "run it as administrator".
  3. **THE FIRST `setup` CAN STALL at "reconciling desktop shortcut (retired-exe repair)"** — that step instantiates
     `WScript.Shell` over COM. It is cosmetic and `setup` is idempotent: **re-run it** (or Ctrl+C, which costs nothing).

**AND THE WHOLE INSTALL IS TWO COMMANDS** (the second one starts the agent):

    npm i -g summrise-agent@1.2.N        # exact version: `latest` can resolve stale and still report success
    summrise.cmd setup                   # components + registry + tasks + the agent

**THE npm PACKAGE CARRIES NO BOXED COMPONENTS — `setup` FETCHES AND VERIFIES THEM.** The package is ~6.7 MB: the exe,
the CLI, the desktop shell's *sources*. `cloudflared.exe` (54 MB), `summrise-playwright.zip` (31 MB) and the **electron
runtime** are served by the release host and staged into `<install>\components` by `resolveComponent()` (`curl -fsSL`, so
an HTTP error is a FAILURE and not a 404 page on disk). Both cloudflared and electron are STAGED IN R2, which is what
lets `index/components.json` pin a sha256 for each and `version.json` publish it: setup REFUSES a mismatch, warns when a
release carries no pin, and never touches GitHub — a device behind the GFW needs no mirror. An *upgrade* was never
affected (components live in `<install>\components` and survive); this bites at install and migration time. The migration
that came up local-only, and why it cost an hour, is in `docs/BRAND.md`.

**MEASURE THE PANEL THE DEVICE IS ACTUALLY RUNNING, not only the harness.** Every design sweep renders the HARNESS (a
stubbed device, this checkout's bundle). `agent/scripts/live-panel-probe.mjs` measures the live panel
(`127.0.0.1:18080`), which is the only way to see a rule no harness run can. It needs a browser and a running panel, so
it cannot be a CI job: run it on the device after a `summrise update`. A 38 KB script is EMITTED, never pasted —

    node agent/scripts/live-panel-probe.mjs --emit > index/public/summrise-agent/live-panel-probe.js
    ./scripts/build.sh index          # A COMMIT IS NOT A DEPLOY: a commit under index/public/ does not change
                                      # what the CDN serves, and the device fetches the PREVIOUS copy
    # the device then fetches it (system_file_download from
    # https://agent.saisi.online/summrise-agent/live-panel-probe.js) and runs it with browser_run_script.
    require('D:/Summrise/live-panel-probe.js');   # REQUIRING IT IS RUNNING IT: the emitted program reads the
                                                  # panel token from the device's config, walks both densities,
                                                  # prints JSON and exits with a verdict code. Nothing is passed in.

After ANY change to a served file, one command says whether the CDN has it:

    curl -s -o /tmp/cdn.js -w '%{size_download}\n' https://agent.saisi.online/summrise-agent/live-panel-probe.js
    cmp -s /tmp/cdn.js index/public/summrise-agent/live-panel-probe.js && echo served || echo STALE

**AND DO NOT `page.goto` THE PANEL FIRST**: the attached view is ONE page shared with the operator's screen, usually
already on the panel, so navigating it aborts (`net::ERR_ABORTED`); the program does its own navigation. **`ATTACHED=false`
IS NOT A SWITCH** — the helper attaches whenever the desktop CDP (`127.0.0.1:9333`) answers, so force headless by
pointing `SUMMRISE_CDP_ENDPOINT` at a dead port, and install `chrome-headless-shell` first
(`node node_modules/playwright-core/cli.js install chromium-headless-shell` — the boxed `playwright-core` does not ship
it and has no `npx playwright`).

**AND THE EMITTED FILE IN `index/public/` MUST NOT BE GITIGNORED: an ignored file is in no other checkout.** Workers
Assets uploads the directory WHOLESALE, `.gitignore` unread, so a worktree deploy REMOVES the missing files from the CDN
(`summrise-agent-latest.tgz` and `SummriseAgent-Setup.exe`: 404). **Deploy `index` from the main checkout**, or copy them
in; the post-deploy smoke catches it.

**TWO THINGS THAT COST A DEVICE RESTART WHEN IGNORED: never launch a second `summrise-agent.exe` from an agent-hosted
PTY** (it inherits the kill-on-close job and kills the running agent), and **never kill/copy the exe inline over a PTY** —
use the npm flow above.

