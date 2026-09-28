# Rust 迁移 · 整体规划

> **一页总览** → 七块判定 → 五个阶段 → 并行规则 → 分支与合入 → 原则 → 防错
> 判据来源：今晚实测 ＋ [secureyeoman 的 TS→Rust 复盘](https://raw.githubusercontent.com/MacCracken/secureyeoman/refs/heads/main/docs/development/migration-finds.md)（77 提交 / 9 天 / 944 路由 / 54K 行）＋ [Cloudflare `workers-rs`](https://developers.cloudflare.com/changelog/post/2025-09-19-workers-rs-panic-recovery/)。
> 防错细节在 `docs/superpowers/plans/2026-09-28-migration-error-prevention.md`。

---

## 一页总览

```
**产品代码现状**：`.rs` **62,854 行**（agent 核心，已是 Rust）· `.ts` 39,517 · `.tsx` 25,459 · `.js` 12,207 · `.mjs` 43,701（门禁＋sweep）

**七块，逐块判**（能不能 → 该不该）：
  ① agent 核心      **已经是 Rust** ✓✓
  ② 面板 UI         **能** ✓ → **做**（统一收益最大）
  ③ 控制台 UI       **能** ✓ → **做**
  ④ 三个 Worker     **能** ✓ → **做**（先量冷启动）
  ⑤ 落地页          **能，已量** ✓ → **最后做或不做**（那一页 4.7× 换 85 行 JS）
  ⑥ 桌面壳 Electron **不迁**（操作者决定：兼容性）
  ⑦ npm CLI         **不能** ✗（npm 的交付方式就是 JS）

**五个阶段**（按时间，不按大小）：
  P0 准备（半天）→ P1 门禁（次要活，与产品并行）→ P2 面板与控制台的逻辑 → P3 三个 Worker → P4 落地页（可选）
  **而产品的五个界面**每一轮都排在这些之前。
```

---

## 七块判定表

| 块 | 路径 | 现在 | 能不能 | 统一语言的收益 | 结论 |
|---|---|---|---|---|---|
| ① agent 核心 | `agent/src/` | **Rust** 62,854 行 | 已经是 | — | — |
| ② 面板 UI | `agent/resources/panel-react/` | TS/TSX ~155 文件 | **能** | **最大**（逻辑多、运行时占比小） | **做** |
| ③ 控制台 UI | `gateway/ui/` | TS/TSX ~23 文件 | **能** | **最大** | **做** |
| ④ 三个 Worker | `gateway/src/` · `index/` · `proxies/` | TS | **能**（wasm） | **大**（服务端） | **做**（先量冷启动） |
| ⑤ 落地页 | `index/src/page.js` | 1 个 `.js` | **能，已量** | **小**（85 行） | **最后或不做** |
| ⑥ 桌面壳 | `agent/summrise-desktop-electron/` | Electron/JS | 不迁 | — | 操作者决定 |
| ⑦ npm CLI | `agent/summrise-agent-npm/bin/summrise.js` | 1 个 `.js` | **不能** | — | npm 就是 JS |

**②③ 的做法**：`wasm-pack build --target web` → 一个 `.js` 胶水 ＋ 一个 `.wasm`，React 里普通 `import`。
**React ＋ Radix ＋ Tailwind 原样留着**——只把真的在做计算的逻辑搬进去。**不重写 UI**（重写会停掉新功能几个月，而 React 能调 wasm，没有必要）。

---

## P0 · 准备（半天）

**做什么**：
1. **出一份清单**：面板与控制台里**哪些是"逻辑"、哪些是"渲染"**（按文件与导出列）。**没有这份清单，P2 会变成"随便挑一个文件改"。**
2. **量基线**：面板与控制台的**体积**、**首屏**、**测试通过数**——**迁移后要拿同一组数对比**。
3. **在 `feat/the-product-moves-to-rust` 上**建一个 `wasm-pack` 的最小可用构建（**只证明工具链通**，不改任何产品代码）。

**判据**：清单存在且可数；基线的每个数旁边有产生它的命令；`wasm-pack` 能在这个仓库里跑出一个空 `.wasm`。
**停止条件**：`wasm-pack` 在这个工具链下跑不通 → **先解决它，不要开始 P2**。
**依赖**：无。

---

## P1 · 门禁 → Rust（次要活，与产品并行）

**做什么**：68 个门禁 → 普查（留/删）→ 搬活下来的 → 全部搬完后删 `.mjs`。
**顺序**：**先搬"真的在解析东西"的**（读源码、走 JSON/YAML、交叉引用两张表）——Rust 的 `serde` **替代**手写解析；**纯减法最后搬或不搬**。
**判据**：每个门禁**同一输入两版结论一致（含失败与边界 ＋ 一个"不该咬"的用例）＋ 变异证明 ＋ `cargo test` 绿**；Rust 版打印**修法**而不只是 `assertion failed`。
**停止条件**：做不到等价 → 留着那个 `.mjs`，提交信息写明为什么。
**依赖**：无（**所以它可以和产品工作同时进行**）。
**样板**：`agent/tests/installer_integrity.rs`。**不新增 CI job · 不新增门禁。**

---

## P2 · 面板与控制台的逻辑 → Rust（统一收益最大的一块）

**做什么**：按 P0 的清单，**一处一处**把逻辑搬进 Rust，编成 wasm，React 里调它。
**每一处的判据**：① 输出与 JS 版一致（同一输入，两个版本对比）② 体积增量 ③ 首屏不受影响（**wasm 只在需要时加载**）。
**停止条件**：某处搬完体积或首屏明显变差 → **留着 JS 版，把数写进提交信息**。
**依赖**：P0。**与 P1 并行**（一个是产品，一个是门禁，互不阻塞）。
**注意**：**一个天真的移植会付、而看不见的代价**——落地页的尖刀实测：把粒子场的 `format!("rgba({},{},{},{:.3})")`（f64）改成整数千分位，**省了 8,879 字节 gz（载荷的 26%）**，因为 `{:.3}` 会把 Rust 的 float formatter 链进二进制。**每一处搬完都要看体积。**

---

## P3 · 三个 Worker → wasm

**做什么**：`proxies` → `index` → `gateway`（最小到最承重）。
**3.0 必须先做**：**量冷启动**——Workers 上 wasm 每 isolate 要实例化，**可能比 JS 慢**。同一个路由，JS 与 wasm 各测 p50/p99 与冷启动。
**判据**：同请求新旧响应**字节可比** ＋ 自己的 smoke 绿（`landing-check` / `production-host-check`）。
**停止条件**：冷启动明显变差 → **只搬 `proxies`**。
**依赖**：P2 的经验（wasm 构建链已通）。

---

## P4 · 落地页（可选，已量）

**已量**：**4.7× 体积**（46,517 gz vs 31,718 B / 9,963 gz）· **首屏一样**（92ms vs 96ms，而 wasm 延后 2 秒仍是 92ms）· **1.6× 行数**。
**可迁移的是架构**：构建时用 Rust 渲染、文字进 HTML、事后 hydrate。
**结论**：**排在最后，或不做**——那一页是 4.7× 换 85 行 JS。
**如果做**：判据是**同一组既有测量**（0 对比度失败 · 最差 4.63 · 阶梯 [16,14,13,12,11] · `costBeforeClick` true · 按钮 rgb(176,58,10)/20px/492×40/白字）。

---

## 与产品工作的并行规则

```
**每一轮的第一个动作**：**五个界面里哪个这轮还没动**——**在打开 CI 之前**。
**2026-09-28 实测：`README.md` 与 `agent/summrise-desktop-electron/` 各是 0 个文件。**
**产品不会喊，机器会喊**——所以产品必须排在日程最前面，否则迁移会把产品挤掉。
**迁移的活排在产品之后**，而 P1 与 P2 可以同时进行（一个是门禁，一个是产品）。
```

---

## 分支与合入

- **一个分支单独迁移**：`feat/the-product-moves-to-rust`（已建、已推、基于 `2840cbb4`）。`ci.yml` 只在 `main` 触发，分支推送**不花 CI**。
- **测试全绿才合入 `main`**，用 `--no-ff`。`main` 只由合并前进。
- **攒批合并**：每次合并是一次 ~7 分钟串行（pre-push 钩子等在途 CI）。**合一次，不要合十次。**
- **推送前先跑 `bash scripts/test/all-gates.bash`**；**在途 CI 时不要推**（推送会取消它，那个提交从此没有绿 CI 可指）。

---

## 五条原则

```
① **一层一层来，每层带测试。**（那份复盘：*"One layer at a time, fully tested"*）
② **旧的在新版被证明等价之前不删。**（那份复盘的 TS 1,726 个文件一个都没删——那是它还能被修的唯一原因）
③ **不再有 ad-hoc 修复提交。** 每处一个分支、一组测试、一次复查。
④ **不量不动手。** 三个"听起来对"的假设已被推翻：**Rust 不会加快 CI**（68 个门禁不在 CI 里，
   最长的是 `pack-chain` 396s、`design` 289s）· **wasm 不会让界面更快**（UI 没有重计算，
   DOM 跨边界反而更慢）· **全部产品代码都该迁是错的**（落地页 4.7× 换 85 行）。
⑤ **产品不等人。**
```

---

## 防错（细节在另一份文件）

**十个真的发生过的错误模式**，每个带证据、防法、以及"怎么验证防法生效"：
静默丢东西 · 等价没证就删 · 跑错环境的命令 · 提交不等于部署 · 一次改太多 · **测量本身是错的** ·
worktree 的三个假前提 · "听起来对"的假设 · 门禁悄悄豁免 · 停在半路没人知道。

**两条元规则**：**每一条防法都要能被证伪**（"小心一点"不是防法，"跑 `curl | cmp`"是）·
**每一条都从真的发生过的错长出来**。


---

## P3.0 的答案（2026-09-28 实测，`47e487f1`）：**冷启动真的更慢，而顺序要改**

**计划里写着 `proxies` → `index` → `gateway` ✓。而那个顺序**没有测量支撑**✗——**现在有了，而它指向反方向 ✓。**

### 数字（Cloudflare 自己的 per-request `cpuTime`，单位 **µs**）

| | TypeScript | Rust→wasm | 比 |
|---|---|---|---|
| 冷 cpuTime p50 | **583** | **9 627** | **16.5×** |
| 冷 wallTime p50 | 965 | 10 230 | 10.6× |
| 暖 cpuTime p50 | 543 | 977 | **1.8×（rust 慢）** |
| bundle gzip | **0.51 KiB** | **141.56 KiB** | **277×** |

**即 **+9.0 ms CPU / 每个新 isolate**✓（18 个配对轮次 ✓，冷 min/p50/max：ts 288/583/935 · rust 846/9627/28441 ✓）。**

**而"wasm 的赢面是 CPU"这条**在这里不成立**** ✗——**暖态 rust 慢 1.8× ✓**，因为这条路由**没有计算可赢**，
**只有边界成本 ✓**。**不要用速度论证迁移 ✓；理由是类型与统一 ✓。**

### 怎么强制冷，以及怎么**证明**它是冷的

`wrangler deploy`（新脚本版本 → 空 isolate 池）然后**恰好一个**请求 ✓。
**而证明不是假设**：两个 worker 都返回 `requestIndex` / `isolateAgeMs` ✓——**`requestIndex === 1` 意味着
**这个请求创建了 isolate**✓**。16 个保存的样本带着它 ✓，确认轮拿到 `isolateAgeMs = 0` ✓✓。

### 而**最重要的那条**：两个仪器不一致，而外面那个**自信地错**

**从外面观察（哪怕从一个**在 Cloudflare 内部**的 driver Worker）会给出一个**自信的错答案**：
冷 p50 ts 69 ms vs rust **61 ms——**rust 看起来更快**** ✗✓。**

**因为那个样本里是网络 ＋ 排队 ＋ isolate 启动（方差 50–90 ms ✓），而效应只有 ~9 ms ✓。**
**答案只出现在 Cloudflare 的 per-request 数据集里——那里**一个只有一次请求的桶就是那个请求**✓✓。**

**所以：量冷启动时，判据是 `workersInvocationsAdaptive` 的 per-request `cpuTime` ✓，
不是任何从外面量的延迟 ✗。**

### 而它对顺序的判断

**惩罚是**按 isolate**✓，不是按请求 ✓——所以暴露量 = **冷请求的比例**✓。**
- **`proxies` 是**流量最低、最突发**的 ✓，而它**在设备的关键路径上**✓（git 镜像 = `proxies/api-relay/api/git.ts` ✓）
  → **它在**最大比例的请求**上付那 9.6 ms ✗**——**所以**不是**先搬它 ✓。**
- **`gateway` 流量最大 ✓（一个真人在用 ✓），摊销得最好 ✓✓。**

**所以 P3 的第一步不是搬任何一个 ✓，是**量每个 Worker 的冷请求比例**✓（`analytics.mjs` 能 ✓），
**然后按那个比例排序 ✓**——**而不是把 `proxies` 先行当成定论 ✗。**

**而上限要说清** ✓：**这是一个 trivial echo ✓——它隔离了运行时（那正是问题问的 ✓），
**但它不预测一条真实路由 ✓。**

### 三条环境事实（都是它踩出来的，别再踩）

1. **`wrangler tail` 在这台机器上跑不了** ✗（ECONNRESET ✓，`tail.developers.workers.dev` 被封 ✓）。
2. **`*.workers.dev` 是 TLS-reset（SNI 被封）** ✗——**从这台机器**和**设备**都不通 ✓，
   **所以流量只能从 Cloudflare 内部发起 ✓。**
3. **`wrangler dev --remote` 起不来** ✗（workerd 要 GLIBC ≥ 2.32 ✓，这台是 2.31 ✓）。
4. **而它**没有用 `wasm-pack`**** ✓——**它用 `worker-build --release` ＋ `WASM_OPT_BIN` ✓，**5.5 秒**✓，
   **没有那 14 分钟的挂 ✓**——**即 `wasm-pack` 那两条警告对 Workers 这条路**不适用**** ✓。


### P3.0 的第二遍测量，更正了第一遍的四条（`7d96b772`）

**同一件事被两个 agent 各量一遍 ✓，而第二遍**更正了第一遍**✓——**而它的结论一样 ✓：**
**冷 **15.5×**（569 → 8 795 µs p50，n=16/17）· 暖 **1.7×**（542 → 940 µs）· bundle **277×**** ✓
（**第一遍是 16.5× / 1.8× ✓——**两个独立测量在同一个量级上 ✓✓**）

**① 而 `isolateAgeMs = 0` **不是**第二个独立确认 ✗✓✓——**它是同义反复**** ✓：
  **"the birth is stamped **ON THE FIRST REQUEST**, so it is 0 exactly when `requestIndex === 1`"** ✓✓——
  **即第一版把**同一个证据**数了两遍 ✓，并把它叫做两个 ✗**——**而原因是**：
  **Workers 在**模块作用域**返回 `Date.now()` 的 **0**** ✓（**measured ✓**）——
  **而 `crypto.randomUUID()` 在模块作用域被直接拒绝 ✓（deploy error 10021 ✓）**
  → **所以"它是冷的"只有一个独立证据：**一次新部署 ✓ ＋ **isolate 自己报的 `requestIndex === 1`** ✓**
**② 而 cron **确实会**触发 ✗✓——**零读数是在**第一个到达之前**取的 ✓**（**~7 分钟 ✓，而它 ~7.5 分钟才启动 ✓**）——
  **它只是**太慢**（~7.5 分钟 ✓），不能当按需触发器 ✓**
**③ 而那个"配对"设计的失败**不只是方差**** ✓✓：
  **"pairing cancels the transport (sd 16.4 vs 50–90 unpaired), yet a **+9 ms penalty still fails to appear**
   (mean −0.5, p50 0). Likely cause is an **ORDER BIAS** the design cannot remove — ts is always subrequest #1,
   rust #2, **so rust starts on a just-warmed DNS/TLS path**."** ✓✓✓——**记录，未修复 ✓：**重跑必须**交替顺序**✓**
**④ `analytics.mjs` 说 cpuTime 的量纲是毫秒 ✗——**它是**微秒**** ✓✓
  （**"543 CPU vs 10 230 wall is only coherent as µs"** ✓）

### 而六条环境事实（都是两遍测量踩出来的）

1. **`wrangler tail` 在这台机器上跑不了** ✗（`tail.developers.workers.dev` 是 TLS-reset ✓）。
2. **`*.workers.dev` 从这台机器**和两台 Windows 设备**都 TLS-reset（SNI 被封）** ✗——
   **所以"side by side"需要一条临时的 `the deployment zone` 路由 ✓。**
3. **而一个 Worker 对**自己 zone**的子请求**不重入 Workers routes**** ✓（**每个样本 522 in 19 ms ✓**）——
   **所以 driver 必须走 `workers.dev` 调目标 ✓。**
4. **8 个并发请求**不会**产生 8 个 isolate** ✗✓（**measured: isolates = 1 ✓——**并发排队到暖 isolate ✓**）——
   **所以**突发不能强制冷 ✓；**一次新部署能 ✓。**
5. **免费版每次调用上限 **50 个子请求**** ✓（**#49 ok ✓，第 50 个抛 "Too many subrequests by single Worker
   invocation" ✓**）——**那给任何重跑定了规模 ✓。**
6. **`wrangler dev --remote` 起不来** ✗（workerd 要 GLIBC ≥ 2.32 ✓，这台是 2.31 ✓）。


### P3.1：每个 Worker 的**冷请求比例**——而它**移除了**那个反对意见（`bce816ec`）

**P3.0 说**"第一步不是搬任何一个 ✓，是量每个 Worker 的冷请求比例" ✓。**而这一节就是那个测量 ✓。**

**窗口 168 小时 ✓，冻结在 `2026-09-28T13:39:00Z` ✓，Cloudflare 自己的 per-request 数据 ✓（**µs ✓**）**：

| Worker | 请求数 | 冷比例 | **× 9.0 ms** |
|---|---|---|---|
| `gateway/` → `vale-gate` | **36 180** | **0.6 % … 2.7 %** | **0.06 … 0.24 ms/req** |
| `index/` → `summrise-dist` | **7 952** | **4.0 % … 6.3 %** | **0.36 … 0.57 ms/req** |
| `proxies/` → **三个**脚本 | **168** | **18.5 % … 51.8 %** | **1.66 … 4.66 ms/req** |
| · `summrise-relay` | 154 | 12.3 … 48.7 % | 1.11 … 4.38 ms |
| · `opencode-go-proxy` | 13 | 84.6 % | 7.62 ms |
| · `zen-us-proxy` | **1** | **100 %** | 9.00 ms |

**而对着每个 Worker 自己的 `cpuTime`/请求** ✓：**`gateway` **+0.4–1.5 %** ✓ · `index` **+43–68 %** ✗ ·
`proxies` **+58–1122 %** ✗**——**而那个才是标题 ✓。**

**而它的**主结论**是**反的**** ✓✓：
- **"the whole penalty is **under 15 s of CPU per week across all three**"** ✓（**2.2–8.7 / 2.9–4.5 / 0.3–0.8 s ✓**）
- **"It is **not a billing problem; it is per-request latency**."** ✓
- **"So: decide P3 on **types and unity** (P3.0's own reason) and **take the blast-radius order** —
  **this measurement's contribution is that it REMOVES the cold-start objection to `proxies`-first**"** ✓✓✓
→ **即：**计划原来的顺序**可以留着**** ✓✓——**而**它当初选那个顺序的理由**（爆炸半径 ✓）也是对的 ✓**——
  **而这一测量的贡献是**它移除了那个反对意见**✓，**而不是改那个顺序**✗✓**
→ **而 168 vs 36 180 是 **215×**** ✓✓（**"1 req per 5.4 h per colo vs 7.5 min" ✓**）

**而"便宜到能搬"的**只有 `gateway`**** ✓✓——**`index` ✗ 与 `proxies` ✗ 都不是 ✓。**

**而上限** ✓✓：**"this is a traffic-shape measurement whose one free parameter is a **distribution,
not a number**"** ✓——**而**isolate 的存活不是一个固定的超时**✓✓**：**同一个 colo（AMS ✓）
出过**37 分钟的幸存者**✓ 和**900 秒内的死亡**✓**——**而那个顺序 `proxies > index > gateway`
**在每一行都成立**** ✓✓（**60 s → 21 600 s ✓**）。

**而它抓到**三处同一个方向的错误**** ✓✓✓——**"All three errors pointed toward the more dramatic answer."** ✓：
1. **第一次扫描（4 个 `curl` ✓，**没有钉 colo**✓）报**2 秒就被驱逐**** ✗——**因为这台机器在 AMS/LHR 之间
   **交替答**✓，而**两个 colo 是两个 isolate 池**✓**——**"that would have put gateway near 100 % cold and
   **reversed the conclusion**"** ✓✓
2. **而它自己的第一稿把**钉住的连接**读成了 isolate 存活 ✗**——**而 `freshconn.mjs` 证明**新连接**确实
   **复用暖 isolate**✓✓**
3. **而结构界第一次数了**每一个**合格桶里的请求 ✓，而只有**第一个**是确定冷的 ✗**——
   **"it said gateway **15.0 %** where the honest floor is **5.1 %**"** ✓

**而它同时更正三条** ✓：**① "`analytics.mjs` 能"**不充分**✗**（**它带的是到达过程 ✓，**不带 isolate 存活**✗**）·
**② "三个 Worker" 是**五个部署脚本**✗✓**（**`proxies/` 是三个 ✓**）·
**③ 而**第六个脚本 `vale-dist`**在账号上而**不在任何计划里**** ✗✓——**`BRAND.md:280` 说它 domainless ✓，
**而它在同一个窗口里服务了 620 个请求**✓**——**即它不是死的 ✓**。
