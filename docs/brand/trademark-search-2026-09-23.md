# Trademark & name-clearance search — "Summrise"

**Verdict (one line):** No mark anywhere queried is identical to `SUMMRISE` — the string `summri` appears in **zero** of TMview's 142,985,928 records and the US register returns 0 for every spelling variant — but **EU trade mark 018936322 `SUMRISE` is a live, registered word mark in classes 9, 35 and 42** (Admisol N.V., BE), which is one repeated letter and an identical pronunciation away from the chosen name in exactly the operator's two core classes.

Observed **2026-09-23** (host date). The file was first written as
`trademark-search-2026-09-27.md` — the date was taken from prose in `docs/BRAND.md` that a
sibling check found to be **four days ahead of the clock** — and it was renamed to
`2026-09-23` once that was caught. Every observation inside is dated the day it was made.

---

## 0. How to read this

**Register data** = a mark number, an owner, a status, a class list, a date, read out of a trademark office's own register or a register-operated mirror. Only this counts as evidence below. It appears in §§1–5 and §8.

**Anecdote / secondary** = search-engine snippets, app-store listings, GitHub repos, HTTP responses from a domain. Useful for context, worthless as clearance evidence. It is quarantined in §10 and never used to support a verdict.

**"Live"** here means what the register says: TMview `tradeMarkStatus: "Registered"` / `"Filed"`, USPTO `alive: true`, EUIPO `status: "Registered"`. It is not a legal opinion and does not account for scope of goods and services, opposition history in national offices, or common-law rights, which were not searched.

**Every query below was run against a machine-readable endpoint, not a web page or a snippet.** Exact requests and responses are in §11.

---

## 1. United States — classes 9, 42, 38

**Endpoint:** `POST https://tmsearch.uspto.gov/prod-stage-v1-0-0/tmsearch` → **HTTP 200**, Elasticsearch `_search` body, **no API key and no WAF token required**. The URL was read out of USPTO's own runtime configuration at [`https://tmsearch.uspto.gov/configuration.json`](https://tmsearch.uspto.gov/configuration.json), which contains `"serviceUrlSearchElastic": "https://tmsearch.uspto.gov/prod-stage-v1-0-0/"` and is what the public Trademark Search UI at [`https://tmsearch.uspto.gov/`](https://tmsearch.uspto.gov/) itself calls.

**Positive controls (proving the query shape works before trusting a zero):**

| Query | Observed |
|---|---|
| `match_phrase` on `wordmark` = `MICROSOFT` | `totalValue: 220` |
| `match_phrase` on `wordmark` = `SUMMERZ` | `totalValue: 1` → serial 98515217, `alive: true`, `IC 044` |

**Exact / near-identical results (`match_phrase` on `wordmark`, all classes, live *and* dead):**

| Mark queried | `totalValue` |
|---|---|
| `SUMMRISE` | **0** |
| `SUMMRIZE` | **0** |
| `SUMMRISER` | **0** |
| `SUMMRIZER` | **0** |
| `SUMMRISES` | **0** |
| `SUMMRIZED` | **0** |
| `SUMMRISING` | **0** |
| `SUMMRISEN` | **0** |
| `SUMRISE` | **0** |
| `SUMMITRISE` | **0** |
| `SUMMIT RISE` | **0** |
| `SUMMARISE` | **0** |
| `SUMMARISER` | **0** |
| `SUM RISE` | **1** → serial 73124210 `WATCH THE SUM RISE`, `alive: false`, `EXPIRED`, `IC 035`, filed 1977-04-25 |

**Conclusion (US):** no identical, and no near-identical, mark. The only hit for any `summ*rise` spelling is a 1977 expired class-35 service mark for the slogan "WATCH THE SUM RISE". **The US register is clear for `Summrise` in classes 9 and 42** on the searches above.

**Closest *live* US marks in the operator's classes** (these are not `Summrise`, and are listed because they are the nearest live commercial neighbours found anywhere in classes 9/42 — see §5): `SUMMIZE INTELLIGENT AGENTS`, US reg. **8148679**, serial 99131111, `alive: true`, `REGISTERED` 2026-02-24, classes **IC 009 / 042 / 045**, owner **Summize Limited** (UK); and `SUMMIZE`, US reg. **6901551**, serial 88432917, `alive: true`, `REGISTERED` 2022-11-15, classes **IC 042 / 045**, same owner. Neither is `Summrise`; both are `Summize`.

**Coverage limits for this endpoint, stated plainly:**
- Wildcard queries on `wordmark` (`summri*`, `*mmri*`, `sumrise*`) all returned **HTTP 400 Bad Request**, so the `summri*` namespace could not be enumerated by prefix in the US index. The zeros above rest on phrase matching, not prefix enumeration.
- My `terms` filter on `internationalClass` **did not discriminate** and is therefore *not* evidence: `AREA SUMMARIZER` (serial 98632276) carries `internationalClass: ["IC 035","IC 009","IC 042"]`, yet filtering `SUMMARIZER` by `["IC 009","IC 038","IC 042"]` returned 0 where the unfiltered query returned 6. Treat the USPTO class-restricted counts as void; every US conclusion above reads the class list off each hit instead.

---

## 2. United Kingdom — classes 9, 42, 38

**UKIPO could not be queried directly.** Every path on the UKIPO search host returned **HTTP 403 with a CAPTCHA interstitial** — from this Linux host *and* from a second machine on a different network path, with a desktop browser user-agent:

| URL | Observed |
|---|---|
| `https://trademarks.ipo.gov.uk/ipo-tmtext` | **HTTP 403**, body `<title>Service Captcha &ndash; Apply to register a trade mark &ndash; Intellectual Property Office</title>` (served by `static.uxforms.com`) |
| `https://trademarks.ipo.gov.uk/` | HTTP 403, same CAPTCHA page |
| `https://trademarks.ipo.gov.uk/ipo-tmcase` | HTTP 403, same CAPTCHA page |
| `https://trademarks.ipo.gov.uk/ipo-tmtext/api/search?searchTerm=summrise` | HTTP 403, same CAPTCHA page |
| `https://trademarks.ipo.gov.uk/ipo-tmowner/...` | HTTP 403, same CAPTCHA page |

**Not worked around.** The UK findings below come from **TMview**, which is operated by EUIPO and carries the UKIPO feed — the same register, a mirror, and I have labelled it as such.

**Result via TMview, office-restricted to GB** (`fOffices: ["GB"]`, all classes):

| Query | Observed |
|---|---|
| `summrise`, criteria C (contains) | `totalResults: 0` |
| `summrise`, criteria E (exact) | `totalResults: 0` |
| `summrise`, criteria F (similar) | `totalResults: 2` |

The two similar marks:

1. **`UK00003902207` — `SUMRISE`** — `tmOffice: "GB"`, `tradeMarkStatus: **Registered**`, `niceClass: [9, 45]`, `applicantName: ["Hugh Carnegie"]`, `applicationDate: 2023-04-18`, `tradeMarkType: "Figurative"`. **A live UK registration covering class 9.**
2. `UK00004278100` — `SUMORISE` — Registered, `niceClass: [22]`, Qingdao Sumshin Outdoor Products Co.,Ltd, filed 2025-10-15. Class 22 only; not a class-9/42/38 concern.

**Conclusion (UK):** `Summrise` itself is unregistered and unapplied-for. But a **live UK registration `SUMRISE` in class 9** exists (figurative, so the verbal element shares the register with a device mark); an identical-word UK mark would be a different question, and it is not one I can answer without the UKIPO register text for the goods specification, which the CAPTCHA blocked.

---

## 3. European Union (EUTM) — this is the finding that matters

TMview (EUIPO-operated) found the candidate; the **official EUIPO register backend** then confirmed it. Both are cited.

**Step 1 — TMview, office-restricted to EM**, `fOffices: ["EM"]`:

| Query | Observed |
|---|---|
| `summrise`, criteria C (contains) | `totalResults: 0` |
| `summrise`, criteria E (exact) | `totalResults: 0` |
| `summrise`, criteria F (similar) | `totalResults: 2` |

**Step 2 — verified against the official EUIPO register** at
`GET https://euipo.europa.eu/copla/trademark/data/018936322` → **HTTP 200**, `application/json`. Observed fields, verbatim:

```
number        = "018936322"
name          = "SUMRISE"
feature       = "Word"
niceclasses   = ["9","35","42"]
status        = "Registered"
rawStatus     = "Registered"
statusCode    = 1
filingdate    = 1696982400000   -> 2023-10-11
regdate       = 1711584000000   -> 2024-03-28
expirydate    = 2012601600000   -> 2033-10-11
applicants    = Admisol N.V., 103 Lousbergskaai, B-9000 GENT, BE,
                legalForm "SA / NV / AG", nationality "BE"
oppositions   = []
cancellations = []
renewalStatus = "Not to be renewed"
publications  = A.1 in bulletin 2023/240 (2023-12-20); B.1 in bulletin 2024/063 (2024-04-02)
```

**What it covers** (class 9 and 42 excerpts, verbatim from the register's `gs` field):

- **Class 9:** *"Software for creating and sharing offers; Software for online administration; … Data management software; Software for point of sales and Software for cash register systems; Software for managing an online shop; Software in the field of logistics and Software in the field of inventory management."*
- **Class 42:** *"Design (development), Writing of computer code, Implementation, hire and Servicing … Software as a service [SaaS] … Platform as a service [PaaS]; Software as a service [SaaS]."*

**Why this is the headline:** `SUMRISE` and `SUMMRISE` differ by a single repeated letter in a position that does not change the pronunciation — both read /ˈsʌmraɪz/. The mark is a **word** mark (no figurative element to distinguish), it is **registered and in force until 2033-10-11**, it has **no oppositions and no cancellations of record**, and it sits in **classes 9 and 42** — software and SaaS — which are precisely the operator's core classes. A EUTM application for `Summrise` in classes 9/42 would run into this mark, and use of `Summrise` in the EU in those classes carries a real likelihood-of-confusion exposure.

**The second EM hit:** `EM/019099571` `SUMORISE` — Registered, `niceClass: [22]`, Qingdao Heshengwei Outdoor Products Co., Ltd., filed 2024-11-01. Class 22 only; not a class-9/42/38 concern.

**Also of record — a lapsed mark that shows the class-38 overlap has existed:** `FR/4166203` `Sumrise` — `tradeMarkStatus: "Expired"`, `niceClass: [9, 35, 38, 41]`, applicant "Ondine, SARL", filed 2015-03-19. Expired, so it is **not** a live bar — but it covered class 38 (telecommunications), which is the third class in scope.

**Re-verified independently, same day, by a second route.** The parent session reproduced both halves through the device's real browser (Chromium, via Playwright). The official EUIPO endpoint returned **HTTP 200, 21,647 bytes**: `"number":"018936322"`, `"status":"Registered"`, `"reference":"TM28965EU00/KB/kbu"`, `"firstlang":"Dutch"`, applicant `Admisol N.V.`, 103 Lousbergskaai, 9000 GENT, BE, `expirydate 2012601600000`. And TMview, driven by a same-origin `fetch()` inside the real page — so the browser carried the F5/traefik cookies itself, no cookie jar needed — returned `GB → SUMRISE | UK00003902207 | Registered | [9,45] | Hugh Carnegie | Figurative` and `EM → SUMRISE | 018936322 | Registered | [9,35,42] | Admisol N.V. | Word`, every field matching this report.

**How it had to be reproduced is itself a finding:** `curl` from the device returned **`000`** and `Invoke-RestMethod` on the same host died with *"the remote host forcibly closed the existing connection"*, while an independent fetch service also failed. By the time of the re-check the register was reachable **only from a real browser** — and only after a Chromium navigation, not a plain HTTP client. That is the fragility §9 item 6 describes, one step further along: the **route** to this evidence is less durable than the evidence.

---

## 4. The `-mmri-` neighbourhood (`Summrize`, `Summriser`, …)

This is the strongest positive result in the search. TMview holds 142,985,928 trademarks from all participating offices; a substring search for the six-character core of the chosen spelling returns nothing at all.

**Positive control first — proving `criteria: "C"` is a true substring search and not a token search:**

| Query (`criteria: "C"`) | Observed `totalResults` |
|---|---|
| `ummerz` | **16** (matches the `SUMMERZ` family) |
| `ummeri` | **121** (e.g. `Hummeri`, `PUMMERIN`, `SUMMERIE`, `nummerix`) |
| `mmris` | **5** (e.g. `MMRISC`, `COMMRISE`, `Pommrische`, `COMMRISK.BM`) |
| **`summri`** | **0** |
| **`ummris`** | **0** |
| **`summris`** | **0** |

**The control and the zero come from the same endpoint in the same session, so the zero is a real absence, not a broken query.**

**Variant sweep** (`criteria: C` = contains, `E` = exact, `F` = similar/algorithmic; all offices, all classes, live and dead):

| Variant | C (contains) | E (exact) | F (similar) |
|---|---|---|---|
| `summrise` | **0** | **0** | 24 |
| `summrize` | **0** | **0** | 19 |
| `summriser` | **0** | **0** | 2 |
| `summrizer` | **0** | **0** | 13 |
| `summrises` | **0** | **0** | 0 |
| `summrized` | **0** | **0** | 2 |
| `summrising` | **0** | **0** | 5 |

**Class-restricted, with a positive control proving the filter discriminates** (`fNiceClass`): control `C("summise")` → 1 hit `CN/SUMMISE[9]`; `fNiceClass: ["9"]` → 1 hit; `fNiceClass: ["28"]` → 0. The filter works. With it applied:

| Query | Classes 9 | Classes 38 | Classes 42 |
|---|---|---|---|
| `summrise` C and E | **0** | **0** | **0** |
| `summrize` C and E | **0** | **0** | **0** |
| `summriser` C and E | **0** | **0** | **0** |

**Conclusion:** **`Summrize`, `Summriser` and every other `-mmri-` variant are not live marks in classes 9, 38 or 42 — they are not marks at all, anywhere in TMview.** The operator's chosen spelling sits one letter from `summarise`, and that homophone family is crowded (§5) — but the `summri*` string itself is unoccupied register space, and the `F` (similar) hits for those variants are `summerize`/`summize`/`summarizer`-type marks, not `summrize`-type marks.

---

## 5. Adjacent live marks in classes 9 / 42 / 38

Not `Summrise`, and none is identical to it — listed because they are the live marks the register puts nearest to the operator's classes and they are what an examiner or an opponent would reach for. All values are register data from TMview unless a USPTO or EUIPO URL is given.

| Mark | Number / office | Status | Classes | Owner |
|---|---|---|---|---|
| `SUMMIZE INTELLIGENT AGENTS` | US reg. **8148679** (serial 99131111) | **REGISTERED**, 2026-02-24, `alive: true` | **IC 009, 042, 045** | Summize Limited (UK) |
| `SUMMIZE` | US reg. **6901551** (serial 88432917) | **REGISTERED**, 2022-11-15, `alive: true` | **IC 042, 045** | Summize Limited (UK) |
| `SUMMIZE` | US serial **50030297** | **Filed**, 2026-08-04, `alive: true` | **IC 042** | Summize Limited (UK) |
| `SUMMIZE INTELLIGENT AGENTS` | `UK00004187333` | **Registered** | **9, 42, 45** | Summize Ltd |
| `summize` | `UK00003287016` | **Registered** | **42, 45** | Summize Ltd |
| `Summerize` | `FR/4353279` | **Registered** | **38, 42, 45** | Summerize, SAS |
| `SUMORIZE` | `IT/2019000069722` | **Registered** | **35, 38, 42** | ARMÒNIAPH SRL |
| `COTOHA Summarize` | `JP/2020039258` | **Registered** | **9, 42** | **NTT Communications** (エヌ・ティ・ティ・コミュニケーションズ株式会社) |
| `AREA SUMMARIZER` | US serial **98632276** | `alive: true`, `REPORT COMPLETED SUSPENSION CHECK - CASE STILL SUSPENDED` | **IC 035, 009, 042** | VALUENEX Japan Inc. |

Two of these deserve a sentence each. **`SUMMIZE INTELLIGENT AGENTS` (US reg. 8148679)** is a live US registration in classes 9 and 42 for downloadable software and SaaS whose own name advertises "intelligent agents" — the nearest live US registration to this product's space, though the mark is `Summize`, not `Summrise`. **`COTOHA Summarize` (JP/2020039258)** is a live class 9/42 registration owned by a telecom operator, NTT Communications — relevant only because the brief asked specifically about telecom-owned marks, and `Summarize` ≠ `Summrize`.

US `SUMMARIZER` search returned 6 marks, of which the only live one is `AREA SUMMARIZER` above; the rest are cancelled or abandoned (`COPERNIC SUMMARIZER` ×3, `PROFESSIONAL SUMMARIZER`, `SYSTEM COST SUMMARIZER`). US `SUMMARIZE` returned 4, all dead. US `SUMMARISE` and `SUMMARISER` returned **0**.

---

## 6. Domains — RDAP

Primary check: `https://rdap.org/domain/<domain>`, following redirects to the authoritative registry server. Where `rdap.org` could not bootstrap a TLD, I went to the registry's own RDAP endpoint, and I checked the IANA bootstrap file to find out whether one exists at all.

| Domain | Endpoint queried | Observed HTTP status | Reading |
|---|---|---|---|
| `summrise.io` | `https://rdap.identitydigital.services/rdap/domain/summrise.io` | **404** | `{"description":["Object not found"],"title":"Object not found","errorCode":404}` → **unregistered** |
| `summrise.dev` | `https://pubapi.registry.google/rdap/domain/summrise.dev` | **404** | `{"description":["summrise.dev not found"],"errorCode":404}` → **unregistered** |
| `summrise.sh` | `https://rdap.identitydigital.services/rdap/domain/summrise.sh` | **404** | `{"description":["Object not found"],"title":"Object not found","errorCode":404}` → **unregistered** |
| `summrise.app` | `https://pubapi.registry.google/rdap/domain/summrise.app` | **404** | `{"description":["summrise.app not found"],"errorCode":404}` → **unregistered** |
| `summrise.cn` | — | **not obtainable** | see below |
| `summrise.com` | `https://rdap.verisign.com/com/v1/domain/summrise.com` | **200** | **registered** — see below |
| `summrise.net` | `https://rdap.verisign.com/net/v1/domain/summrise.net` | **404** (empty body) | → **unregistered** |
| `summrise.co` | — | **not obtainable** | see below |

**`summrise.com` is registered.** RDAP shows `ldhName: SUMMRISE.COM`, registrar **IONOS SE**, events `registration 2015-01-09T14:48:51Z`, `expiration 2027-01-09T14:48:51Z`, `last changed 2026-01-10T08:23:02Z`, status `["client transfer prohibited"]`, nameservers `ns1031.ui-dns.biz / ns1059.ui-dns.de / ns1062.ui-dns.com / ns1075.ui-dns.org`, `secureDNS.delegationSigned: false`. No registrant is disclosed (redacted for GDPR, as expected).

**`summrise.cn` and `summrise.co` — RDAP could not answer, and I will not pretend otherwise.**
- `https://data.iana.org/rdap/dns.json` (the RDAP bootstrap file) contains **no entry for `.cn` or `.co`** — the only TLDs in scope that have one are `com`/`net` (Verisign), `app`/`dev` (Google), and the bootstrap has none for `.io` or `.sh` either (which is why `rdap.org` answered `{"title":"No RDAP service is available for this resource"}` for those four — that message is a *bootstrap miss*, **not** evidence of availability, and I have not treated it as such).
- `https://rdap.nic.co/domain/summrise.co` → no TCP connection (HTTP 000; `getaddrinfo ENOTFOUND` from a second host).
- `https://rdap.cnnic.cn/domain/summrise.cn` → no TCP connection (HTTP 000; `getaddrinfo ENOTFOUND` from a second host).
- No `whois` client is installed on this host, so the WHOIS fallback was not available either.

**So: `summrise.cn` and `summrise.co` are UNVERIFIED.** The only signal I have is secondary (§10).

**RDAP cannot tell you whether a registered domain is parked or in use.** It carries no such field. For `summrise.com` I therefore report the register facts above and, separately and clearly labelled as *not* RDAP, the DNS/HTTP observation in §10.

**DNSSEC note:** `secureDNS.delegationSigned: false` for `summrise.com` per RDAP.

---

## 7. npm

All checks against the registry itself (`https://registry.npmjs.org`).

| Name | URL | Observed |
|---|---|---|
| `summrise` | `https://registry.npmjs.org/summrise` | **HTTP 404** `{"error":"Not found"}` → **free** |
| `summrise-agent` | `https://registry.npmjs.org/summrise-agent` | **HTTP 404** → **free** |
| `summrise-cli` | `https://registry.npmjs.org/summrise-cli` | **HTTP 404** → **free** |
| `summrise-cli-agent` | `https://registry.npmjs.org/summrise-cli-agent` | **HTTP 404** → **free** |
| `@summrise/agent` | `https://registry.npmjs.org/@summrise%2fagent` | **HTTP 404** → **free** |
| `@summrise/cli` | `https://registry.npmjs.org/@summrise%2fcli` | **HTTP 404** → **free** |
| `@summrise` scope / org | `https://registry.npmjs.org/-/org/summrise` | **HTTP 404** `{"code":"ResourceNotFound","message":"/-/org/summrise does not exist"}` |
| `@summrise` scope / org (v1) | `https://registry.npmjs.org/-/v1/org/summrise` | **HTTP 404** `{"code":"ResourceNotFound", ...}` |
| any package in scope `@summrise` | `https://registry.npmjs.org/-/v1/search?text=scope:summrise` | `{"objects":[],"total":0}` |
| any package matching `summrise` | `https://registry.npmjs.org/-/v1/search?text=summrise` | `{"objects":[],"total":0}` |

**Conclusion:** `summrise` is unregistered and the `@summrise` scope holds nothing. One caveat on method: an unauthenticated lookup of `https://registry.npmjs.org/-/user/org.couchdb.user:summrise` returns **HTTP 401** (that endpoint requires auth), so it is inconclusive on its own — the conclusion rests on the `/-/org/summrise` and search results above, which are conclusive in the negative direction. npm scope names are also not a trademark register and carry no legal weight.

---

## 8. Anything that would kill the name outright

The brief asked for a live mark in classes 9/38/42 owned by a telecom or software company, a well-known brand, or the same name on a competing product.

- **No well-known company brand collision with `Summrise`.** Nothing in the searches above resembles a household brand, and no mark `SUMMRISE` exists anywhere I could query.
- **No identical mark owned by anyone**, telecom or otherwise.
- **The nearest thing to a kill shot is EUTM 018936322 `SUMRISE`, classes 9/35/42, Admisol N.V. (BE)** — a software company's registered word mark, one letter and no audible difference from the chosen name, in the operator's two core classes, in force to 2033. It is not identical, so it does not *kill* the name in the way an identical live class-9 mark would; but it is a material EU/UK exposure and it is the fact the next decision should be made against.
- **`SUMMIZE INTELLIGENT AGENTS`, US reg. 8148679, classes 9/42/45** (Summize Limited, UK) is the nearest live *US* registration in the operator's classes. Different mark, same `summ`-stem, same "agent" space.
- **A telecom-owned mark does exist in classes 9/42 — `COTOHA Summarize`, JP/2020039258, NTT Communications — but its mark is `Summarize`, not `Summrize` or `Summrise`,** and it is a Japanese registration.

**Nothing found is disqualifying in the sense of an identical live mark in classes 9/38/42.** The EUTM in §3 is the one finding that changes what the operator should do next.

---

## 9. What I could not see

This section is the honest half of the report. Every gap below is a place where a live conflicting mark could be sitting unobserved.

**Registers and endpoints I could not query at all**

1. **UKIPO — `trademarks.ipo.gov.uk`. HTTP 403 CAPTCHA on every path, from two different hosts, with a browser user-agent.** The page title observed is `Service Captcha – Apply to register a trade mark – Intellectual Property Office` (assets served from `static.uxforms.com`). There is **no direct UKIPO register search in this report**; the UK row in §2 is TMview's UKIPO feed. It is a register-operated mirror, but it is a mirror, and I have not seen the UKIPO goods specification for `UK00003902207` — only TMview's class list `[9, 45]` and status `Registered`.
2. **USPTO Open Data Portal — needs a key.** `https://api.uspto.gov/api/v1/trademarks/search` → **HTTP 403** `{"message":"Missing Authentication Token"}`. Same for `/trademarks/status`, `/tm-search`, `/trademark/search`, `/trademarks`. Not worked around.
3. **USPTO TSDR API — needs a key.** `https://tsdrapi.uspto.gov/ts/cd/casestatus/sn<serial>/info.json` → **HTTP 401**.
4. **WIPO Global Brand Database — CAPTCHA-gated.** `https://branddb.wipo.int/en/` returns HTTP 200 but loads `https://cdn.jsdelivr.net/npm/altcha/dist/altcha.min.js` (an Altcha proof-of-work CAPTCHA); every API path probed (`/branddb/api/search`, `/api/search`, `/branddb/rest/search`) returns the SPA shell, not JSON. **Not queried.** WIPO's Madrid data would have widened international coverage beyond the offices TMview carries.
5. **EUIPO eSearch plus *search* — no public search endpoint found.** The per-application detail endpoint (`/copla/trademark/data/<number>`) answered HTTP 200 and is used in §3, but `/copla/trademark/search`, `/copla/search/trademark` and `/eSearch/api/search` all returned 404 or an Apache placeholder. **The EU search was therefore done through TMview (EUIPO-operated) and the hit verified against the official EUIPO register by application number** — found via the mirror, confirmed at the source. A search that TMview's index missed would not have been seen.

**Network reachability (this is why the route above mattered)**

6. **`www.tmdn.org` (109.232.208.203) and `euipo.europa.eu` (109.232.208.177) are unreachable from this Linux host** — DNS resolves, TCP 443 times out on both (`curl: Connection timed out after 15000/30000 ms`). Both were reached instead through the operator's Windows device on a different network path. If that device's egress changes, **TMview coverage — which is the backbone of §2, §3 and §4 — disappears.**
7. **`bulkdata.uspto.gov`, `data.uspto.gov`, `assignment-api.uspto.gov`** → HTTP 000 (no connection) from this host. USPTO bulk data and assignment records were **not** searched, so **ownership changes and recorded assignments are unexamined.**

**Queries that returned a zero I cannot fully stand behind**

8. **USPTO class-restricted counts are void.** The `terms` filter on `internationalClass` failed its positive control (see §1). The unfiltered US searches stand; the class-restricted ones are reported only so the failure is on the record.
9. **USPTO wildcard/prefix enumeration is unavailable** — `wordmark` wildcards return HTTP 400. I could not enumerate `summri*` in the US index; the US zeros rest on phrase matching.
10. **USPTO `prod-stage-v1-0-0` is an undocumented internal-looking gateway.** It answered HTTP 200 with no key and no WAF token despite the public SPA loading an AWS WAF challenge (`https://a434627cf98f.edge.sdk.awswaf.com/.../challenge.js`) and an Okta configuration (`CLIENT_ID 0oa82ydoy3u3bKeD94h7`, `ISSUER https://auth.uspto.gov/oauth2/aus462q76dSh7EUHU4h7`). It could change or be withdrawn without notice; **re-running these US queries later may require a different route.**

**Things this search did not attempt at all**

11. **Common-law / unregistered use.** No search of app stores, GitHub, company registers, or the open web was used as evidence. The brief asked whether the name is *on a competing product's packaging*; I answered the register half of that and did **not** do the marketplace half, because a search snippet is not evidence for a clearance question.
12. **Classes 35 and 45**, which keep appearing in the hits (the EUTM covers 35; the UK `SUMRISE` covers 45; `SUMMIZE` covers 45), were outside the brief and were not searched systematically.
13. **Opposition proceedings, national office decisions, and scope-of-goods analysis** were not searched. `oppositions: []` for EUTM 018936322 means none recorded against *that* mark — it says nothing about what its owner might oppose.
14. **`.co` and `.cn` availability is UNVERIFIED** (§6) — no IANA RDAP service exists for either TLD, the registries' RDAP hosts do not resolve, and no `whois` client is installed here.
15. **RDAP cannot distinguish a parked domain from an active one.** Nothing in §6 should be read as saying a domain is "in use".

---

## 10. Secondary observations (not evidence)

Labelled as anecdote per the rules. **None of this supports the verdict.**

- **`summrise.com` HTTP behaviour.** `http://summrise.com` → **HTTP 200**, 336-byte body, no `<title>`, no visible text. `https://summrise.com` → **HTTP 000** (no TLS). DNS: `A 217.160.0.78`, `MX 10 mx00.ionos.co.uk / mx01.ionos.co.uk`, `TXT "v=spf1 include:_spf-eu.ionos.com ~all"`. The nameservers, MX and SPF are all IONOS, consistent with a domain held on an IONOS account. **This is a DNS and HTTP observation, not RDAP, and it does not establish parking** — an empty 200 with no title is equally consistent with a placeholder, a disabled vhost, or a host that serves content only on a path.
- **`.io` / `.sh` / `.cn` / `.co` all have no nameservers and no A record** in DNS, which is consistent with the RDAP "not found" answers for `.io` and `.sh` and is the only signal available for `.cn` and `.co`. A registered-but-undelegated domain would look the same, so this corroborates but does not prove.
- **TMview's own disclaimer**, displayed on the page: *"TMview 与 DesignView 检索工具不构成官方注册，工具中所包含的信息不具备任何法律效力"* — TMview and DesignView are not official registers and the information carries no legal effect. This is why §3 verifies the EUTM against EUIPO directly.
- **TMview reported 142,985,928 trademarks** across participating offices at the time of search (page heading), and version `TMDSVIEW-4.5.1-RC1.1`.

---

## 11. Appendix — exact queries and observed responses

Reproducible commands. "device" = run from a machine whose network can reach `tmdn.org` / `euipo.europa.eu` (this Linux host cannot; see §9 item 6).

### US — USPTO trademark search index (no key)

```bash
curl -s -X POST "https://tmsearch.uspto.gov/prod-stage-v1-0-0/tmsearch" \
  -H "Content-Type: application/json" \
  -d '{"query":{"match_phrase":{"wordmark":"SUMMRISE"}},"size":30,
       "_source":["id","wordmark","alive","statusDescription","internationalClass",
                  "ownerName","registrationId","filedDate","markType"]}'
# -> HTTP 200  {"took":..,"hits":{"totalValue":0,"hits":[]}}
# config read from: https://tmsearch.uspto.gov/configuration.json
```

### TMview (EUIPO/TMDN) — search

```bash
# The cookie jar is REQUIRED and must be shared between the two calls: the API
# resets the connection (ECONNRESET) if the POST arrives without the F5/traefik
# cookies that the warm-up GET sets. `-c`/`-b` is what made this reproducible.
curl -s -c /tmp/tm.cookies -b /tmp/tm.cookies "https://www.tmdn.org/tmview/" -o /dev/null
curl -s -c /tmp/tm.cookies -b /tmp/tm.cookies \
  -X POST "https://www.tmdn.org/tmview/api/search/results" \
  -H "Content-Type: application/json" -H "Origin: https://www.tmdn.org" \
  -H "Referer: https://www.tmdn.org/tmview/" \
  -d '{"page":1,"pageSize":30,"criteria":"F","basicSearch":"summrise","fOffices":["EM"]}'
# -> HTTP 200, totalResults: 2, the first element containing (observed verbatim):
#    "tmName":"SUMRISE"  "tmOffice":"EM"  "applicationNumber":"018936322"
#    "tradeMarkStatus":"Registered"  "niceClass":[9,35,42]
#    "applicantName":["Admisol N.V."]  "tradeMarkType":"Word"
#    "applicationDate":"2023-10-11T12:00:00.000Z"
# Response envelope keys observed on this endpoint: tradeMarks, page, totalPages, totalResults.
```
`criteria`: `C` = contains (substring — verified, see §4), `E` = exact, `F` = similar.
Filters `fOffices` and `fNiceClass` were both verified to discriminate against positive controls (§4).
`GET` on this path returns **405 Method Not Allowed** — it is POST-only.

### EU — official EUIPO register, by application number

```bash
curl -s "https://euipo.europa.eu/copla/trademark/data/018936322" -H "Accept: application/json"
# -> HTTP 200 {"number":"018936322","name":"SUMRISE","feature":"Word",
#              "niceclasses":["9","35","42"],"status":"Registered","statusCode":1, ... }
```

### Domains

```bash
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.io"        # 404
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.dev"       # 404
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.sh"        # 404
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.app"       # 404
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.com"       # 200
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.net"       # 404
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.cn"        # 404 "No RDAP service is available"
curl -sL -o /dev/null -w '%{http_code}\n' "https://rdap.org/domain/summrise.co"        # 404 "No RDAP service is available"
# authoritative per-registry, used where rdap.org could not bootstrap:
curl -sL "https://rdap.identitydigital.services/rdap/domain/summrise.io"   # 404 Object not found
curl -sL "https://rdap.identitydigital.services/rdap/domain/summrise.sh"   # 404 Object not found
curl -sL "https://pubapi.registry.google/rdap/domain/summrise.dev"         # 404 summrise.dev not found
curl -sL "https://pubapi.registry.google/rdap/domain/summrise.app"         # 404 summrise.app not found
curl -sL "https://rdap.verisign.com/com/v1/domain/summrise.com"            # 200 registered
curl -sL "https://rdap.verisign.com/net/v1/domain/summrise.net"            # 404
curl -s  "https://data.iana.org/rdap/dns.json"                             # no .cn, .co, .io, .sh entry
```

### npm

```bash
curl -s -o /dev/null -w '%{http_code}\n' "https://registry.npmjs.org/summrise"          # 404
curl -s -o /dev/null -w '%{http_code}\n' "https://registry.npmjs.org/summrise-agent"    # 404
curl -s -o /dev/null -w '%{http_code}\n' "https://registry.npmjs.org/summrise-cli"      # 404
curl -s "https://registry.npmjs.org/-/org/summrise"                                     # 404 does not exist
curl -s "https://registry.npmjs.org/-/v1/search?text=scope:summrise"                    # total 0
```

---

*Prepared 2026-09-23. Register data only for the conclusions; everything else is quarantined in §10. Gaps are in §9 — read it before relying on any zero above.*
