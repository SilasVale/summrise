# Vitals history: the device card's memory and disk over time

> **For agentic workers:** Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** the device card's live memory/disk facts gain a **browser-local history**: a small sparkline
of available memory % and the tightest disk's free % over the last two hours, sampled from the device
list the console already polls.

**Architecture:** web-only. The console already receives `host` facts with every device in
`GET /fleet`; the App samples them on its existing refresh (at most once per 30 s per device), keeps a
bounded history in `localStorage` (one key per device), and passes it to the DeviceCard, whose host
block renders an inline SVG chart. No device, console-server or API change.

**Tech Stack:** React 19 + TS, the existing `DeviceCard`/`HostFactsBlock`, vitest/jsdom/RTL.

**Premise check (done):** the device card renders `host.memory`/`host.disks` live, and there is no
vitals/history surface in the page (`grep -rni "vitals|history" device/console/web/src` finds only the
runs/audit histories). This adds history, not a duplicate view.

## Global Constraints

- **Honest coverage.** The history exists only while a console page is open in this browser; the
  caption says exactly that, plus "2 h / 30 s". Gaps are real and are not interpolated.
- **Bounded.** At most 2 hours and 240 samples per device; the oldest are dropped; the key is
  `summrise.vitals.<deviceId>`. A corrupt/foreign value loads as empty, never throws.
- **Only real facts.** A sample requires an **online** device with `host.memory.total_bytes` and at
  least one disk with a positive total; an offline device or a missing host adds nothing (no stale
  flatline). The disk series is the **tightest** disk (lowest free %), named in the label.
- **One sample per 30 s per device,** even though the App polls faster; the App's existing refresh is
  the only sampler (no new timer). A failed `localStorage` write (quota/private mode) loses history,
  never the page.
- **No secrets.** The stored value is memory/disk bytes and the timestamp; no ids of tokens, no
  names, no grant material.
- **Accessible without the chart.** The latest values and the coverage are text; the SVG is
  `aria-hidden`. The chart is additive; a device with no samples shows the collecting sentence.
- English; `npm run typecheck`, `npm test`, `./scripts/build-web.sh` (exit code, bundle committed).

## Review Focus

1. **The sampler is a pure function with injected time.** `recordVitals(devices, now)` decides due/
   retention/cap; tests fake `now` and storage; the App calls it with `Date.now()` after a successful
   device refresh.
2. **Storage failures are contained.** Quota/security errors are swallowed; a corrupt JSON value
   yields an empty history; another device's key is never read.
3. **The chart is honest.** The caption names the window, the cadence and the "while a page was open"
   limitation; one sample says "collecting"; the disk label names the disk chosen.
4. **The tightest disk is the story.** A device with one disk charts it; with several, the lowest free
   percentage is the one drawn and named.
5. **No duplicate power.** No new intervals; the sample cadence is enforced by the stored last
   timestamp. The App test's existing poll counts are unchanged.

---

### Task 1: The history

**Files:** create `device/console/web/src/vitals.ts` + `vitals.test.ts`; create `VitalsChart.tsx` +
`VitalsChart.test.tsx`; modify `DeviceCard.tsx`, `App.tsx`, `app.css` (+ their tests).

**Interfaces:**
- `type VitalsSample = { ts_ms: number; memory_available_bytes: number; memory_total_bytes: number;
  disk_name: string; disk_free_bytes: number; disk_total_bytes: number }`.
- `recordVitals(devices: DeviceRef[], now: number): Record<string, VitalsSample[]>` where
  `DeviceRef = { id: string; online: boolean; host?: HostFacts | null }`; loads/stores through
  `localStorage`; at most one new sample per device per 30 s; retains 2 h / 240.
- `loadVitals(id: string): VitalsSample[]` (exported for tests/DeviceCard).
- `VitalsChart({ samples }: { samples: VitalsSample[] })`: `<2` samples → the collecting sentence;
  otherwise two `polyline`s (memory available %, tightest disk free %) plus text labels, the latest
  values, and the coverage caption.
- `DeviceCard` prop `vitals?: VitalsSample[]` (default `[]`), passed to `HostFactsBlock`, which renders
  `VitalsChart` after the live memory/disk rows. App keeps `vitals` state from `recordVitals(list,
  Date.now())` in `refreshData` and passes `vitals[device.id] ?? []`.

- [ ] **Step 1: Tests first** (vitals sampler, chart, DeviceCard/App)
- [ ] **Step 2: Implement, gates, commit**
  `console: the device card keeps a two-hour vitals history`.

### Task 2: The record

- [ ] Tick this plan, record the rulings (why localStorage, why 30 s/2 h/240, why the tightest disk,
  why no server-side sample), commit `docs: the vitals history's decisions`.

## Self-review

- Spec: host facts exist live; the operator's next question is "what were they an hour ago", and the
  console already polls the facts.
- Review Focus: pure sampler, storage containment, honest caption, tightest disk, no new timer.
- Deliberate: browser-local (survives reloads, not closed browsers); no server sampling this round; no
  chart library; memory + tightest disk only (not every disk or CPU).
