# Ideas inbox — the user's

Anything in here is the loop's TOP-PRIORITY work, and it is the anchor against
drift: round 100's "aurora" was the loop *interpreting* a request instead of
following one, and the user had to say plainly that they never wanted it.

Rules for the loop:

- Entries arrive here written by the user, or said in the session (append them
  here so they survive the round).
- Each entry gets either a round of its own, or an explicit reasoned verdict in
  the round log — with evidence — if the loop judges it not worth doing.
- Never dropped silently. Never reinterpreted into something else.
- `docs/CHARTER.md` is the user's alone; the loop may only propose changes.

| # | Idea (the user's words) | Status | Where it landed |
|---|---|---|---|
| 1 | The project should iterate and design itself; my creative direction enters through here | standing | `docs/CHARTER.md` + `docs/agents/iteration-loop.md` |
| 2 | 我不是说了断舍离吗，这种没有用的功能加了干嘛啊 | done | The `monitor_note` tool, its route, its panel rendering and its CLI projection were DELETED (rounds 32-33), and so were `vale watch\|report\|monitor wait\|monitor note`. Verified in round 61: no `monitor_note` remains anywhere under `agent/src/`. |
| 3 | 加这个按键条干嘛 | done | The console key bar (`ConsoleKeys`), its lib, its tests and its CSS were removed; verified in round 61: zero references in the panel. |
| 4 | 可以做个新手引导之类的现在功能我都不知道怎么用 | done | `GettingStarted.tsx` + `lib/gettingStarted.ts`: three steps, reopenable from the rail's `?`, version-keyed dismissal. Verified present in round 61. |
| 5 | 有些多余的功能学会舍弃或者合并 | done | The Archive and Activity pages were merged behind one rail icon (round 31); the key bar and `monitor_note` deleted. |
| 6 | 不要经常使用sleep，使用后台任务监听不好吗 | done | Long waits are background jobs with `job_output`; sleeps are only used where a wall-clock boundary is the thing under test, and the one place that needed a longer wait (the panel's 30s poll) is measured by fake timers instead. |
| 7 | 你能全用英文吗，goal目标也同时改成英文 | standing | Session language is English, and the goal text is English (`goal-905e1213`, revision 28 at round 61). |
| 8 | goal加一个多花些心思在页面设计上面 | done | The design axes are measured, not asserted: contrast (rendered probes + a static pair sweep), hierarchy/landmarks, focus and hover states, geometry at 320-1440px, WCAG reflow, accessible names, boot/interaction timing — packaged as `agent/scripts/*-design-sweep.mjs` with gates. Rounds 39-60. |
| 9 | 跑完这轮先暂停 (said in this session, round 54) | done, then resumed | The goal was paused at revision 27 immediately after that round finished; it was resumed afterwards (revision 28), so the loop is running again. |
| 10 | (the loop's ask, recorded here because it needs YOU) | waiting on you | One `vale update` on d1. The device runs 1.2.403; everything through 1.2.432 is published and audited, and none of it is visible there. The update restarts the agent and closes the 16 open sessions, so the loop will not run it unasked. |
