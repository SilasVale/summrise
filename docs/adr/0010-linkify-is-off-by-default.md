# ADR 0010 — path linkification is OFF by default, because it rewrites nodes the host owns

Date: 2026-09-14 · Status: Adopted

## Context

The `Vale Code Links` extension turns path mentions in a chat reply into links to the
code-server folder. It does so by replacing the text node it found with a `<span>`
containing the same text plus `<a>` elements.

The host (the DSH client) renders replies with React and reconciles its markdown text
nodes **in place**: the same source-offset keys, no remount, and its next streaming
chunk writes to the node it already holds (`MarkdownText` in the host's client
package). So after this extension replaces a node, the framework's next write goes to
a detached node: the reply **freezes** at the injected link, and a later structural
diff or unmount can throw `NotFoundError` inside React's commit phase. The extension's
README claimed the opposite — that replacing the node wholesale was what kept streaming
from fighting it — and the claim had never been tested against a real stream.

This is a known-unsafe technique, not a bug to be patched: the host's contract is node
identity, and any rewrite from outside breaks it.

## Decision

Path linkification is **off by default** and enabled only by an explicit opt-in
(`studioLinksEnabled === true`; the previous default was on, and the previous read was
`!== false`, i.e. opt-out). The README states the tradeoff instead of claiming safety,
and this ADR records why.

The detection work is kept: the pure guards, the workspace allowlist and the refusal
rules are tested and are what a host-side renderer will need.

## Rejected options

* **Keep it on by default and document the risk.** A feature whose measured effect is
  corrupting the host UI does not belong on by default, and the corruption is silent —
  it looks like the model stopped generating.
* **Remove the extension's linkification entirely.** It is useful to a user who accepts
  the tradeoff on a page that does not stream, and removal would throw away working,
  tested detection logic that the real fix needs.
* **Patch the rewrite to be React-safe.** Not possible from outside: the framework
  reconciles by node identity, so *any* replacement breaks it. Marking nodes, cloning
  them or re-inserting into a React-owned slot all leave the framework pointing at the
  old node.

## Consequences and measurements

* Externally observable metric: with the setting off, a streamed reply must contain no
  injected node and must not freeze. Failure criterion: links appear while the setting
  is off (the gate failed), or a reply freezes with the setting on (the tradeoff is
  worse than documented).
* The gate is pinned by a test that reads the content script's default and opt-in
  predicate (the content script cannot be imported — it talks to `chrome.*`).
* **Deletion criterion**: delete the default-off gate, its pin and this ADR when
  linkification is rendered **host-side** (a DSH client plugin, or the host's own
  mention API). At that point nothing rewrites a framework-owned node and the tradeoff
  disappears.
