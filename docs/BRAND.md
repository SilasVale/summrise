# BRAND — Summrise

> **Named 2026-09-27 by the operator**, on sound: *「我觉得 summrise 很好听，就用这个。」*
> **Renamed completely the same day** (decision ①: every layer, including the ones the
> installed device reads, accepting **one device reinstall** as the price of leaving no
> remnant behind).
>
> This file is the brand's single source of truth: what the name means, how it is said,
> the risks that were known when it was chosen, **what the rename actually changed**,
> and — the one list that must never be "cleaned up" — **what was deliberately left
> alone**.

## The name

| | |
|---|---|
| Product name | **Summrise** |
| Pronunciation | `/ˈsʌmraɪz/` — **SUM-rize**, two syllables |
| Structure | `summ`(summit 的缩写)+ `rise` — the [noun]+[verb] shape *sunrise* uses |
| Command | `summrise` (was `vale`; the `caprise` dual-bin went with the old name) |
| Chinese name | **峰起** (candidate — see §Open) |
| Tagline (EN) | *The summit rises into first light.* |
| Tagline (CN) | 山顶先见光。 |

### What it means

`summ` clips **summit** — the same clipping statistical tables use (`Summ.` = Summit
League). `rise` is the verb the sunrise family uses. The compound does not describe the
sun's motion; it describes **the high point coming up into the light**: the peak is lit
while the valley is still dark.

Three readings, in the order they arrive:

1. **Natural** — the summit rising into first light;
2. **Product** — the operator stands at the end that is first visible and first reachable;
3. **Morphological** — it looks and sounds like the sunrise family (`sunrise`,
   `moonrise`, `earthrise`), so half the meaning arrives before anyone explains it.

### Pronunciation — SUM-rize, and why the collision is survivable

`/ˈsʌmraɪz/` is **two** syllables. *summarise* is **three**: `/ˈsʌməraɪz/`. Spoken aloud
the two are distinguishable — the brand is missing a syllable, not a letter. The
collision with *summarise* is therefore **orthographic, not phonetic**: it costs
something in writing and search, nothing in speech.

Standardise it in public text: `Summrise /ˈsʌmraɪz/ — SUM-rize, like sunrise`.

Chinese stays **意译, never 音译** — 音译 would be 萨姆赖兹 / 苏姆莱兹, unstable and
surname-shaped.

### What is known about the name (measured 2026-09-27)

| Layer | Reading |
|---|---|
| Software | 12 GitHub repositories named `summrise`, highest **★1**. **No software product carries the name.** |
| npm | `summrise`, `summrise-cli`, `summrisehq` — all unregistered. |
| Domains | **`summrise.io` / `.dev` / `.sh` / `.app` / `.cn` — all five free.** Only `summrise.com` is taken. |
| App Store | **One same-name app: *Summrise*** (SIMLAB, 2025-07-09, "AI-powered learning assistant … summarizing YouTube videos"). The `-mmri-` family also holds `Summrize — AI Summaries` (Chrome) and `AI-Summriser` (GitHub). |
| Companies | No software company found under the name. |
| Trademark | **UNVERIFIED.** No register search has been run. The largest open risk. |

### Accepted risks — recorded because the choice was made with them known

1. **Orthographic collision with *summarise*** — one letter short of one of the most
   frequent verbs in software, and the name of the category that App Store app sits in.
   Cost lands on writing and search; speech is unaffected. Mitigations: own the search
   term (all five domains free today), and always print the category beside the name —
   *Summrise, the Windows device agent*.
2. **A same-name App Store app exists** — an iOS learning assistant, where this is a
   Windows agent distributed over npm. No platform overlap, no competitive conflict; the
   cost is search ambiguity only.
3. **`summ` is a written abbreviation, not a lexical clipping** — English pronounces
   `/sʌm/` as *some* and *summer*, not as *summit*. Cost: the meaning needs one sentence
   the first time. Judged acceptable; **the sound decided it.**

## The rename — what actually changed (2026-09-27)

Three token-aware passes: **431 files rewritten, ~7,500 occurrences, 29 paths renamed**.
Nothing below is aspirational; each row was verified afterwards.

| Surface | Before | After |
|---|---|---|
| GitHub repository | `SilasVale/vale` | `SilasVale/summrise` (old path 301-redirects) |
| Checkout directory | `~/vale` | `~/summrise` |
| Rust crates | `vale-agent`, `vale-agent-core` | `summrise-agent`, `summrise-agent-core` |
| Binary | `vale-agent.exe` | `summrise-agent.exe` |
| npm package | `vale-agent` | `summrise-agent` |
| CLI command | `vale` | `summrise` |
| CDN paths | `/vale-agent/*` | `/summrise-agent/*` |
| Cloudflare workers | `vale-gate`, `vale-dist` | `summrise-gate`, `summrise-dist` |
| Environment variables | `VALE_*` (15) | `SUMMRISE_*` |
| Registry key | `HKLM\SOFTWARE\Vale\Agent` | `HKLM\SOFTWARE\Summrise\Agent` |
| Install / data dirs | `C:\Program Files\Vale`, `C:\ProgramData\Vale` | `…\Summrise` |
| Markers | `.vale-release`, `vale-agent.hostname` | `.summrise-release`, `summrise-agent.hostname` |
| MCP server name | `vale-gate` | `summrise-gate` |
| Device cookie | `vale_pt_<device>` | `summrise_pt_<device>` |

### Consequences of touching the device layer (accepted, not discovered later)

- **The old CDN paths are gone.** A device on the old version cannot update itself — it
  asks for `/vale-agent/vale-agent-latest.tgz`, which no longer exists. **Every installed
  device must be reinstalled, not updated.**
- **The old npm package name is gone**, so the old `vale` CLI cannot pull the new one.
  The order is: old CLI out, new package in (§Open; **not yet executed on any device**).
- The old registry key, service and data directory are **not migrated** — the old install
  is removed first, which is exactly what leaves nothing behind.

### How it was executed, so it can be reviewed or repeated

- **Token-aware, never substring.** The repo contains 42 English words carrying the
  string `vale` — `equivalent` (34), `valence` (5), `valet` (3). A `sed
  s/vale/summrise/g` would have produced `eqisummrisent`. Pass 1 used ordered literal
  forms plus `\b` word-boundary rules; pass 3 **masked the protected words first** and
  then replaced every remaining occurrence — which is what makes the result zero
  remnants rather than zero *visible* remnants.
- **The masking had exactly one bug, and a separate check found it:** the protected word
  `valet` also swallowed `valetestboundary` in `index/test/upload.test.mjs`. The
  over-masking query (protected-word *prefixes*) returned three tokens, all real
  leftovers, now fixed.
- **`cargo fmt --all` must run after the rename.** `use summrise_agent::…` sorts
  differently from `use vale_agent::…`, so the first rebuild died on
  `cargo fmt --check` over import order in `agent/tests/*.rs` — a formatting failure
  that reads like a build failure.
- **Build artifacts were regenerated, not hand-edited**: `panel.js` (vite),
  `bin/summrise.js` and the electron `dist/` (tsc). The rebuilt `panel.js` contains **0**
  occurrences of the old name.

**Verification, all green (2026-09-27):** `cargo xwin check --target
x86_64-pc-windows-msvc --features terminal,keyring` exit 0 · npm package `npm test`
44/44 · panel `npm test` 814 passed / 103 files · gateway `npm test` 917/917.

## What was deliberately NOT renamed — the complete list

| Left alone | Why |
|---|---|
| `docs/agents/design-ledger.md`, `docs/agents/ideas.md` | Those rounds happened while the project was called Vale. Mechanically replacing the name would erase the fact, and a rewritten record is not a record. |
| `.superpowers/sdd/*.diff`, `task-7-report.md` | A diff is a diff *of a commit*: editing one changes its content while its hashes still point at the original. Rewriting cryptographically pinned history is corruption, not tidying. |
| `agent/deploy/retired/*` | Retired installers, kept as the record of what was shipped. |
| The ~60 published `*.tgz` in the repo | Their sha256 is recorded in the CDN manifest; renaming a file changes its identity and would break the audit forever. |
| `LICENSE:3` — `Copyright (c) 2026 SilasVale` | That is the **account** name, and accounts are not renamed: GitHub's redirect for a *repository* is reliable, for a *user* it is not, and once the old account name is taken by anyone else every old clone, link and release URL breaks at once. |
| `ACCESS_TEAM_DOMAIN=vale-saisi.cloudflareaccess.com` (`gateway/wrangler.jsonc`) | A Cloudflare Access **team domain is an account resource that already exists**. The rename produced `summrise-saisi.cloudflareaccess.com`, which resolves to nothing: a gateway deployed with it fails every JWT check and **authentication dies for everyone**. Renaming the team is a Zero Trust dashboard action that invalidates existing sessions — until that is taken deliberately, this var keeps the real domain. Caught by diffing the wrangler configs against the live account, not by a test. |
| `equivalent`, `equivalence(s)`, `valence`, `valet` | English words that merely contain the string. |

**Two more names are real resources, and the rename did re-point them** (so they are
*not* in the table above, but they are not free either): the **R2 bucket** became
`summrise-temp-files`, which `wrangler deploy` auto-provisioned as a **new, empty**
bucket — the old one holds only 24 h relay temp files, so nothing durable was lost; and
the **Cloudflare Access team** name, handled above.

## The runner-up, recorded so the choice is reconstructable

**`caprise`** — *cap* (the snow cap, a summit's top layer) + *rise*; legal morphology, no
same-name app (one same-name App Store *publisher*, Caprise Ltda, shipping unrelated
apps), `.io`/`.dev`/`.sh`/`.cn` free. It lost on **sound, not on evidence**, and it stays
here as the fallback if the trademark search comes back bad — which is why it was not
deleted once `summrise` had been executed everywhere.

## Rejected — and the reason each one died

| Candidate | Died of |
|---|---|
| `daybreak` | Daybreak Game Company LLC — live US marks, active litigation; `daybreak` taken on npm |
| `morn` | **Homophone of *mourn*** (morning/mourning). Unfixable by spelling. Also 摩恩 = Moen |
| `lumin` | Magic Leap's **Lumin OS**; Lumin PDF; 长安 Lumin; 3 same-name App Store apps |
| `sol` | SOL = Solana's ticker; Sol beer; Sol de Janeiro. Three letters of Latin are unregistrable |
| `solgan` | One transposition from **slogan** — reads as the most common typo in English |
| `sunrise` | Sunrise UPC (a national telecom, same class) + SUNRISE Inc. (Gundam) + **9 same-name App Store publishers**. 11,247 repos |
| `matins` | US application (Matins Group Inc, 99671375), class unknown; **Mattins/Matins** splits every search |
| `alba` | A UK **class-9** consumer electronics brand (televisions via Argos), Alba Botanica, the ALBA party |
| `oriens` | Two **live EU registrations** (018995977, 018577988); a Chinese class-9 `ORIEN`; class-38 ORIENS CREATIVE; a same-name software company |
| `anatol` | `-ol` reads as a chemical/drug in English; an established given name; 2,130 repos (★745 theme) |
| `oriana` | 4 syllables, shifting stress; in Chinese 奥莉安娜 is a League of Legends champion |
| `primerise` | `-ise`/`-ize` splits it; it means "apply primer" or "add a PCR primer" |
| `forthrise` | `forth` is a particle, not a noun — illegal compound; one letter from **forthright** |
| `fortrise` | Reads as FORT + rise (a fortress); one letter from **fortress** |
| `solune` / `solume` | Coined, and **already registered**: solune holds a UK mark (UK00004166864) and a US application |
| `earthrise` | US application (Earthrise Ventures LLC, 97618480) |

## How these numbers were measured — and what each instrument cannot see

Recorded because four readings in the session that produced this file were wrong before
they were checked: a GitHub "taken" column that was really HTTP 403 (the core API quota
had run out at `0/60`); a claim that `caprise` had no product using it, taken from search
snippets rather than a registry; a claim that `solune` was clear, made before the
trademark layer was looked at; and a "reads as *caprice*" judgement that the actual usage
data contradicted.

| Claim | Instrument | Cannot see |
|---|---|---|
| npm names | `registry.npmjs.org/<name>` (404 = free) | — reliable |
| GitHub crowding | search API `q=<name> in:name` | substring noise — `sunrise` matches 11,247 rows, most of them `sunriseSunset`-style |
| Domains | RDAP (`rdap.org/domain/<d>`, 404 = unregistered) | whether a registered name is *in use* |
| App Store | public iTunes Search API (`itunes.apple.com/search?term=…&entity=software`) | Windows, web, Steam; and it matches loosely |
| Trademark | **nothing yet** — web search only, which is not a register search | everything that matters. The open row. |

## Deployment state (2026-09-27, end of the rename)

**Live and verified.**

| Thing | State |
|---|---|
| CDN worker | `summrise-dist`, with the download host and the command host (`<download-host>`, `<command-host>`) attached to it |
| CDN content | `/summrise-agent/version.json` → `1.2.452`; `/summrise-agent/summrise-agent-latest.tgz` → 200 (6,690,379 B); `/vale-agent/*` → **404**, which is the design: the reinstall *is* the migration |
| Release | `summrise-agent-1.2.452.tgz` published; landing page 200 |
| Repo | three commits pushed — `9acb2db1` (the rename), `28887dd4` (the Access fix), `24bcc049` (prettier + mirror) |

**Two infrastructure names keep the old spelling. Neither is a brand surface.**

1. **`vale-gate` — the Cloudflare worker name.** Worker secrets are *write-only*:
   `vale-gate` holds **15** of them (`ADMIN_PASSWORD`, `SESSION_SECRET`, `DO_AUTH`,
   `UPLOAD_KEY`, `API_HOST`, `CLIENT_KEY`, `CMD_API_KEY`, `R4_API_KEY`, and seven upstream
   model keys). A new worker name starts with none, and the seven upstream keys cannot be
   invented. Deploying `summrise-gate` fresh would therefore not rename the gateway — it
   would **disable the model gateway and change the console password**. So
   `gateway/wrangler.jsonc` was set back to describe reality: the worker stays
   `vale-gate`, and the gateway and AI hosts (`<gate-host>`, `<ai-host>`) stay attached to it — untouched,
   zero downtime. (If the 15 values are ever re-entered, the rename is a two-line change
   plus a domain move.)
2. **`vale-saisi.cloudflareaccess.com` — the Access team domain**, recorded in the table
   above.

Both are the same lesson as the account name: *a string that names a live resource holding
state we cannot copy is not ours to find-and-replace.*

**Left running on purpose as rollback — delete when the new CDN has served a day:**

```bash
wrangler delete vale-dist                          # no domain points at it any more
wrangler r2 bucket delete vale-temp-files          # empty; only 24h relay temp files lived there
```

**Out of this repo's scope, still carrying the old name** (separate components, not
renamed): `~/vale-forge` + `~/.local/bin/vale-forge` + `~/.vale-forge.json` — the MCP
client behind the `vale-gate` server label — and the label itself in
`~/.dsh/profiles/web/cordis.patch.yml`. Renaming that label renames the live MCP tool
namespace, so it is a client-restart action, not a rename side effect. Also
`~/vale-deploy`, `~/vale-signing-test`, `~/vale-stage-l-*`.

**The one action left that only a human can take: reinstall the device.**

Everything that could be prepared was prepared on 2026-09-27, from this session, while
the old agent kept running — the steps below therefore only have to be *run*:

| Already done on the device | Where |
|---|---|
| the old data dir copied (minus the 159 MB `pwout/` scrape scratch) | `C:\ProgramData\Summrise\` — `memory/`, `runs/`, `sessions/`, `logs/`, `monitors.json` |
| the two config files renamed to what the new code looks for | `summrise-connections.json` (your SSH/serial connections), `summrise-known-hosts.json` |
| the identity and config staged, with the label updated | `C:\ProgramData\Summrise\migrated-from-vale\`: `config.yaml` (carries `device_token` and `console_url`; `server.name` rewritten `vale-agent` → `summrise-agent`), `tunnel.yml`, `summrise-agent.hostname` (= `d1.<download-host>`) |

```powershell
# 1. the new CLI (the running old agent is untouched)
npm i -g --prefix (Split-Path (Get-Command npm).Source) <download-host>/summrise-agent/summrise-agent-latest.tgz
# 2. new registry key, D:\Summrise, C:\ProgramData\Summrise   (setup does NOT write config.yaml)
summrise setup
# 3. put the identity back BEFORE the new agent's first boot, or it registers a second device
$e = (Get-ItemProperty 'HKLM:\SOFTWARE\Summrise\Agent').InstallDir + '\etc'
Copy-Item 'C:\ProgramData\Summrise\migrated-from-vale\*' $e -Force
# 4. remove the OLD agent -- the channel into this box drops here, which is expected
Stop-Service ValeCommand -ErrorAction SilentlyContinue ; vale uninstall
# 5. start, then confirm release 1.2.452 AND that the console still shows the SAME device
summrise autostart ; summrise start ; summrise status
# 6. only once it is healthy:
Remove-Item D:\Vale,C:\ProgramData\Vale -Recurse -Force
```

The shipped install command lives in `agent/AGENTS.md` (Release) and names the real host;
it is deliberately not duplicated here. `--prefix` matters: without it npm installs
elsewhere, reports success, and `summrise update` ships the old exe.

**Rollback** (the old tgz is still served from the CDN): install
`<download-host>/summrise-agent/vale-agent-1.2.451.tgz` and run `vale setup`.

**It was deliberately NOT run from the session that did the rename.** That session reaches
the device *through the agent being replaced*, and this repo's own notes record that
killing or relaunching the agent from an agent-hosted PTY kills the running agent. A
device whose agent is removed while it is the only way in is a device nobody can finish
installing. Until it is reinstalled the device keeps working on the old build; it simply
cannot update itself, because the path it would ask for returns 404.

## Open

- **Trademark** — US/UK/EU, classes 9/42/38. The largest open risk, and the reason
  `caprise` is kept above as a fallback.
- **Claim today**: npm `summrise` (a placeholder — a coined name's whole value is being
  first; `solune` was coined in the same session and is already registered in the UK), and
  the five domains `summrise.io` / `.dev` / `.sh` / `.app` / `.cn`, all free.
- **Chinese name**: 峰起 (recommended — 峰 = summit, 起 = rise) vs 山起 vs 云起.
  「顶升」is out on two counts: a registered mark being traded, and a heavy-machinery
  term (顶升法, 液压顶升).
