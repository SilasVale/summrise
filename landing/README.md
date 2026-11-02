# The landing page

The public face of the product: `public/` is the whole site, deployed as static assets on
Cloudflare Workers at **https://summrise.saisi.online**.

## Why this is not Rust

The repository's rule is "prefer Rust", and the console's pages are Rust because they are rendered
from the product's own state. This page is the opposite: it has no state, no session and no
authentication, and its whole job is to be a document a CDN can serve without a process. A Rust
server here would add a runtime, a deploy and a failure mode to buy nothing. What the rule buys —
a check that runs where the other end runs — is `check.mjs`, wired into `ci.yml`'s `landing` job.

## What is here

```
public/
  index.html         the page: English in the markup, so it renders without JavaScript
  assets/site.css    the stylesheet; tokens match device/ui so the two cannot drift apart
  assets/site.js     the language switch, the newest-release lookup, the sticky-nav hairline
  assets/lang-zh.js  the Chinese strings, one dictionary
  assets/og.png      1200x630 social card, rendered from og.html
  _headers           cache policy: the page revalidates, the assets may sit for an hour
check.mjs            the gate: translations, anchors, assets, contrast, social card
render-og.sh         re-renders public/assets/og.png from og.html
wrangler.jsonc       the Worker: static assets only, plus the custom domain
```

## Preview it

```bash
python3 -m http.server 8080 --directory public --bind 127.0.0.1
# http://127.0.0.1:8080/          English (the default)
# http://127.0.0.1:8080/?lang=zh  Chinese
```

Loopback only, deliberately: no process in this repository listens on a non-loopback socket.

## Check it

```bash
node check.mjs
```

It fails on: a string that exists in only one language (either direction), a duplicate id, an anchor
that points at nothing, a referenced file that is missing, a missing title or description, a
placeholder left in the page, a text colour under WCAG AA 4.5:1 on the surface it is actually drawn
on, or a social card that is not 1200x630.

The contrast list is not decoration. Before it existed, `--faint` measured **3.31:1** on the paper —
the meta line, the kickers and the captions, all of them small text. The measurement is what moved it
to `#6f6a60` (4.90:1), and re-introducing `#8a867d` fails the gate with that number in the message.

## Change the copy

English is the default and lives in `index.html`; Chinese lives in `assets/lang-zh.js`. A translatable
element carries `data-i18n="key"`, and the gate refuses a key on only one side:

1. add `data-i18n="section.thing"` to the element in `index.html`, with its English text as content;
2. add `"section.thing": "……"` to `lang-zh.js`.

HTML is allowed in both: the switch swaps `innerHTML`, so `<br>`, `<em>` and the brand's
`<span class="sun">` survive translation.

## Nothing is a hardcoded number that can be fetched

The download block asks `api.github.com` for the newest **published** release and fills the version,
the installer and the checksum links from it. If the API is unreachable, the links point at the
releases page and the version line says so — a page that prints `0.3.152` because somebody typed it
is a page that lies after the next release.

**Known gap, measured 2026-10-28:** the newest published GitHub release is `v0.3.12` (a prerelease),
while the fleet runs `0.3.152` from the console mirror. The page is honest about what it links to;
publishing the current build is a separate step, and until it happens the download button leads to
releases that are older than the product.

## Render the social card

```bash
./render-og.sh          # needs Chrome/Chromium; writes public/assets/og.png
```

## Deploy

```bash
CLOUDFLARE_API_TOKEN=$(cat ~/.cloudflare-token) \
CLOUDFLARE_ACCOUNT_ID=8e9ea6cb01f2336a2f00039a51f96c6d \
wrangler deploy
```

`wrangler.jsonc` carries the custom domain (`summrise.saisi.online`, `custom_domain: true`), so the
DNS record and the certificate are created by the deploy. The `workers.dev` URL also exists; on a
network that poisons `workers.dev` it is not reachable, which is why the page has its own name.
