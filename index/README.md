# Summrise Index — download landing page

A tiny Cloudflare Worker serving `agent.saisi.online`: the download landing
page for summrise-agent (npm-only channel since 2026-08-28) and the static
release artifacts (versioned npm tgz + `summrise-agent-latest.tgz` alias +
`version.json` discovery manifest). Device management lives in the Summrise
console — this worker only distributes install artifacts.

## Deploy

```bash
cd index
CLOUDFLARE_API_TOKEN=$(cat ~/.cloudflare-token) npx wrangler deploy
```

Custom domain **agent.saisi.online** is bound in the Cloudflare dashboard
(`workers_dev: false`).

## Publishing a release (the ONLY supported path)

```bash
./scripts/publish-release.sh 1.2.N
```

from the repo root. It packs the npm tgz, stages it + the `latest` alias,
writes `version.json` (with the sha256 agent_update requires), prunes old
versions to the last 5 (round-309), commits, and deploys this worker. Then
push main and create the GitHub tag `v1.2.N` via the API — release.yml
builds the GitHub release asset.

Static assets in `index/public/` are served first via the `ASSETS` binding;
everything else hits the Worker (`/api/version` derives the update manifest
from `version.json`, round-297).

## Installing summrise-agent on a machine

```bash
npm i -g https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise setup            # pure local; --reg-key <key> registers with a console
```

To UPDATE an already-installed device, add `--prefix` — without it npm installs
into its default global prefix, which is not where `summrise` lives when the agent
runs as SYSTEM, and `summrise update` then silently stages the OLD exe:

```powershell
npm i -g --prefix (Split-Path (Get-Command summrise).Source) https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz
summrise update
```

## The installer asset

- `/summrise-agent/SummriseAgent-Setup.exe` is SERVED when a release publishes one — it is an
  ASSET, not a redirect, and the worker's route reads it from the bucket (`src/index.js`).
  **THE NSIS INSTALLER IS NOT RETIRED**: `scripts/publish-release.sh` builds it on every release
  (the pack step) and the manifest's `installer` field decides whether the landing offers it — an
  absent field is a PUBLICATION STATE, not a retirement. `scripts/build.sh` carries the full
  history of a comment that said otherwise for long enough that a reader would have believed it.
