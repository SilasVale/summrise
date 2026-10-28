# The product story, and the landing page it implies

> **Status: decided and shipped, 2026-10-28.** The operator chose the light, product-page direction
> (`b-light`): English by default with a Chinese switch. It is built in `landing/` and deployed at
> **https://summrise.saisi.online**. The four directions in this directory stay as the record of
> what was rejected and why; each is a self-contained HTML file, so the record costs no binaries.

## One sentence

> **Summrise turns a Windows machine into an AI-operable workspace — and gives its owner one console to
> see, approve, and take back control from anywhere.**

The second clause is not decoration. Every product in this space can give an agent a shell; the thing this
one sells is that handing the machine over stays a **decision**, not a leap of faith.

## The four nouns

Everything the product does is one of these. If a feature cannot be stated as one of them, it is a
delivery detail.

| noun | what it is | what it replaces | the cost when it is wrong |
|---|---|---|---|
| **Device** | a real Windows machine running the runtime: one SCM service (`summrise-launch`) supervising one app (`summrise-device`) | a sandbox/container that is not the machine the work belongs to | an update that cannot be recalled, or a machine that cannot repair itself |
| **Session** | something the AI can drive: terminal, SSH, serial, browser, files, monitors, runs | a transcript that claims work happened | "it said it ran it" with no evidence |
| **Grant** | the signed, scoped, revocable permission behind every call | an API key with ambient authority | an agent doing something nobody can attribute |
| **Reachability** | how an operator gets to the device: the console from anywhere, the panel from the machine itself | a per-device tunnel and a public hostname | a device that is unreachable exactly when it is wanted |

## The value ladder

Four rungs, in the order an operator feels them.

1. **Hands.** The agent operates the real box — its network, its signed-in browser, its COM ports — not a
   lookalike. This is the capability no chat window has.
2. **Brake.** Each call is a grant and an audit row; destructive tools are refused unless the token allows
   them; the operator can hold the keyboard and the agent waits; revocation is one action. This is what
   turns "an agent with hands" into something a person will actually allow.
3. **Operations.** The launcher owns the app binary: it verifies at boot, repairs what is missing or
   corrupt, and swaps only against a signed release document. A bad network is a design input, not an
   exception.
4. **Distance.** The device dials out; nothing listens inbound. One console sees the fleet from anywhere.
   The panel at `127.0.0.1` still works when the console does not — and it is the only surface that can say
   *why* the console cannot.

## What it is not

- Not a cloud IDE or a sandbox: the work happens on the machine the work belongs to.
- Not remote desktop: a person does not watch a person; an agent works and a person can intervene.
- Not multi-tenant SaaS: one operator, a handful of machines, each enrolled deliberately.
- Not a per-device public hostname: the tunnel-per-device shape is what this replaces.
- Not an Electron/npm stack: one launcher, one app, one console, Rust throughout.

## Proof points the page may cite

Only numbers that come from somewhere. A landing page that hardcodes a count is a page that drifts.

- two online machines on `0.3.152`, console on `0.3.145`, all updates signed and verified;
- enrollment proven on both sides (console protocol and the device's own CLI);
- every console page is Rust-rendered and render-compared (28 pages, light and dark) on every UI change;
- the device's panel is loopback-only and has no login; the console is behind Cloudflare Access or an
  operator token;
- 504 rate after the proxy fixes: 1 in 78, attributable to a deploy window (before: 4 in 6).

## What the page has to do

1. **Say what it is in ten seconds** — hero, one sentence, one real screenshot of the console.
2. **Answer "is it safe to hand over"** — the control/audit section is the buying objection, not a
   footnote. This is where the product differs from every "give your agent a shell" page.
3. **Get to a download in one click** — installer, checksums, release document, and a five-minute quick
   start. The download is the conversion; everything else supports it.

## The three directions

| file | the argument it makes | shape |
|---|---|---|
| `a-editorial.html` | calm, documentation-adjacent, developer-credible | light paper, big type, one framed console shot, feature grid |
| `b-dark.html` | the agent is already running; this is an engineering product | near-black, warm glow, terminal card, hairline feature rows |
| `c-controlroom.html` | control is the product; the page should feel like the console | warm paper, orange, the console bleeding off the right edge, "watch it work / take it back" |
| `b-light.html` | **chosen** — the product page in daylight: the terminal is the proof, the fleet strip is the scope | warm off-white, dark terminal card, two-column capability grid |

## Open decisions (the operator's)

1. **Direction** — which of the three (or which combination) becomes the page.
2. **Language** — the repository is English; the operator is Chinese. A Chinese page and an English page
   are different products to different audiences; this needs an answer, not a default.
3. **Where it lives** — its own hostname, a public route on the console, or a replacement for the old
   `agent.saisi.online` landing page.
4. **Download channel** — GitHub Releases (what the installer workflow uploads today) or the console's own
   mirror, which is what a device uses to update.
