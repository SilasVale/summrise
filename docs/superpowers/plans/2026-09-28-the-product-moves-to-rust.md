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


### P2 的转折点：**异步边界那条路用尽了**（`5aa635d2`）

**而这一节是 P2 的**下一步的判据**✓——**因为它把"接着搬哪一个"变成了"选哪条已经算过价的路" ✓✓。**

**① 而那个 agent 核了**每一个剩下的大族的调用点** ✓——**而它们**全是渲染路径**✓✓**：

| 族 | 而它的调用点 |
|---|---|
| `path.ts` 的 `derivePath` / `attentionSteps` | `PathView.tsx:104-105` 的 `useMemo` |
| `liveness.ts` 的 `deviceLiveness` / `sessionLiveness` / `sessionWaiting` / `anyCommandRunning` | 组件渲染 |
| `runs.ts` 的 `groupOperation` | `RunStrip.tsx:133` 的 `useMemo` |
| `operationRows` | `ActivityPage.tsx:184` 的 `useMemo` |
| `useCommandEvents.ts` 的 `groupEvents` | `:483` 的 `useMemo` |
| `useTrajectory.ts` 的 `groupRounds` | `:102` 的 `useMemo` |
| `disambiguateLabels` · `cardState` | 渲染 |

**而问题是** ✓：**wasm 是在**第一次调用**时取的 ✓——**所以渲染期间的**同步**调用**等不到它**✓✓**。

**② 而两条路它都**算过价**** ✓✓：

- **① 内联字节 → **+18,913 gz = 首屏载荷（273,252 gz）的 6.9%**** ✗——**"and it GROWS with every
  family" ✗**（**wasm 的 base64 是 43,448 raw / 18,869 gz ✓，加上胶水的增量 ✓**）——
  **而按实测的边际（每族 ~1.5–2.7k gz ✓），它是一个**只会变坏**的固定选择 ✓**——
  **"It cannot be the answer for the render-path majority."** ✓
- **② 在数据边界派生 → **0 gz**** ✓✓——**而那是计划自己那句**"parse 去 Rust ✓，`useEffect` 留着"✓
  **的**下一层**✓✓**——**即：**把派生从 `useMemo` 挪到**数据边界**✓（**那里 wasm 已经加载了 ✓**），
  **而让组件读那个结果 ✓**。

**③ 而它同时留下两条可复用的判据** ✓✓：
- **`key` 是 `` `${id}:${atMs}` `` ✓——一个 JS 的**数字→文本**转换 ✓，而 Rust 的 `format!` 说
  `1000000000000000000000` 而 JS 说 `1e+21` ✓——**而一个拼法不同的 key 是一个**不同的 key**✓，
  于是"重复的转换会**堆叠**而不是替换自己"✓**——**而那正是 `key` 存在的理由 ✓✓**。
  **修法：问那个**引擎**✓（`Number.prototype.toString` via `js_sys` ✓），**永远不要用 Rust 的
  formatter** ✗**——**而那与那个 8,879 gz 的 `{:.3}` 教训是**同一个修法**✓✓**。
- **而它**拒绝了一个豁免清单**✓✓**：`wire-field-check` 把 `logic.parse_monitors(j)` 读成了一个
  wire 字段 ✓，而它的信息里提供了 `NOT_DEVICE_FIELDS` ✓——**而它拒绝了**✓：
  **"because the migration adds one export name per family and **an exemption list that grows with it
  is a gate being quietly exempted**"** ✓✓——**而它改的是**那个正则**✓**（**跳过一个**被调用**的名字 ✓，
  因为**设备送的是数据 ✓，从来不是一个函数**✓**）——**而那个门禁**仍然报同一个断言**✓**（**20 个字段 ✓**）


### 而**路 ② 有一个前提，而这一轮撞了它两次**（2026-09-29 ✓）

**上面那张表说的是每个族**在哪里被调用**✓——而路 ② 假设它们**每一个都能挪到数据边界**✓。**
**而这一轮核了两个族 ✓，**而它们各自用不同的方式**不满足那个前提**✓✓：

| 族 | 而它为什么不满足 |
|---|---|
| `disambiguateLabels`（`lib/sessionLabels.ts` ✓） | **它是**子集**的函数，而子集由调用点选 ✓✓**：`TabBar.tsx:71` 传 `sessions` ✓，`DesktopShell.tsx:173` 传 `openTabs` ✓——**而碰撞集**不同**✓，所以**一次**边界派生会**改掉其中一个**✓（在全部会话里编号 ≠ 在打开的标签里编号 ✓） |
| `pruneLagMarkers`（`lib/lagMarkers.ts` ✓） | **它**改共享状态**✓✓**：`useSSE.ts:147` 在**传输循环**里删 `lagBackfill` 的条目 ✓——**而 `await` 在那里是**竞态**✓**：wasm 第一次取要过网络 ✓，而那个窗口里到达的帧读到的是**还没被剪的**标记 ✓ |
| `terminalStatus`（`useCommandEvents.ts:83` ✓） | **它在一个**同步 fold** 里面 ✓✓**：`:141` 在 `for (const ev of events)` 里 `cards.push(finishCard(… term.exitCode …))` ✓——**要 await 它，就得把**整个 fold** 变成异步**✓ |

**所以"挪到数据边界"不是一条**通用**的路 ✓，而它需要**三个前提**✓✓：**纯 ✓ · 结果**不被同一段同步代码接着读 ✓ · 且是**整个数据集**的函数而不是调用点选出来的子集 ✓**。
**而按这三条筛，上面那张表里的 `path.ts` ✓、`liveness.ts` ✓、`runs.ts` ✓ 都**可能**过 ✓（它们读的是整个 devices/sessions/runs ✓），
而 `disambiguateLabels` **明确不过** ✓。**

**这一节改的是**下一步做什么**✓**：**不是"挑一个族然后挪到边界"✓，而是**先按这三条给那张表分类**✓✓**——
**过的走边界（0 gz ✓）✓，不过的只有三条路 ✓：把**两个子集各派生一次**✓（两次调用 ✓，语义不变 ✓）、
**内联字节**✓（**+18,913 gz ✓ 而它只会变坏 ✓**）、或**留在 TS**✓。**

### P4 的判据，**在线上逐位复现**（2026-09-28 ✓）

**而这一节记的是：P4 的那组数，**在设备上、对着线上那个页**，被重新量了一遍 ✓——**而它们**逐位相同**✓✓。**

| P4 的判据 | 线上实测 | |
|---|---|---|
| **0 个对比度失败** | **`contrastFails: 0`** | ✓ |
| **最差 4.63** | **`worst: 4.63`** | ✓ **逐位** |
| **阶梯 `[16,14,13,12,11]`** | **`ladder: [16,14,13,12,11]`** | ✓ **逐位** |
| **那句诚实话** | **`"No Windows installer is published for this release"` 在 ✓** | ✓ |
| **wasm 只在需要时加载** | **`hasWasm: false` · `resources: 2`** | ✓ **那是设计** |

**而它同时记下一件更重要的事：我的第一次测量**是错的**** ✗✓✓。

**第一版脚本报 `contrastFails: **10**` · `worst: **1.24**`** ✗——**而 P4 说 0 与 4.63 ✓**——
**而那个错的版本**自己带着否证**✓✓**：
- **那个背景是**半透明的**✓：`bg: "color(srgb 0.980392 0.980392 0.980392 / **0.84**)"`** ✓——
  **而我的脚本往上找背景时**停在它上面**✗**——**即它把一个半透明的卡片当成了背景本身 ✗**
- **而它报的第二条**不可能是真的**✓：`"npx summrise-agent setup"` · `cr: **1.24**` ·
  `color: **rgb(29,29,31)**`** ✓——**那是一个代码块 ✓，而近黑配浅色是**高对比**✓**
→ **而修法只有一行** ✓✓：**把每一层背景**按 alpha 合到底**✓（`over(fg, bg)` ✓）——
  **而改对之后，它**逐位复现了 P4 的判据**✓✓。**
→ **即：**那个页没变 ✓，而**我的工具**变了 ✓**——**而那正是防错计划第 6 条的形状 ✓✓**：
  **"测量本身是错的" ✓（**"31140 是 UTF-16 单元，真值 31,718" ✓**）——
  **而一个**看起来对的**测量 ✗ 会报出**十个不存在的失败**✓，**而它的数据里**永远有否证**✓。**


### P4 的第五条判据**缺一个主语**（2026-09-28 ✓）

**而这一节记的是：P4 那一行**读起来像"这个页的按钮" ✗——**而它不是 ✓**。**

```
**P4 原文** ✓：**"0 对比度失败 · 最差 4.63 · 阶梯 [16,14,13,12,11] · `costBeforeClick` true ·
  **按钮 rgb(176,58,10)/20px/492×40/白字**"** ✓——**而它把五条判据**并列**✓，
  **于是那个"按钮"读起来像**落地页上的一个按钮**✗✓**
**而实测（2026-09-28 ✓，设备上，对着线上那个页 ✓）**：
  · **落地页上**没有**那个按钮** ✗✓——**最接近的是 `"Summrise console"` ✓，
    而它是 **`112×20`** ✓，**不是 `492×40`** ✓**
  · **而 `buttons: []`** ✓（**过滤 `w > 60 && h > 24` ✓）——**即**那个尺寸的按钮**不在此页**✓**
→ **即：那条判据的主语是**另一个页面**✓（**很可能是那个 console 的登录页 ✓**）——
  **而它被写在 P4 里 ✓，而 P4 说的是**落地页**✓**——**于是它**读错了地方**✗✓**
```

**而同一轮量到的**其余四条**都对** ✓✓**：

| P4 的判据 | 线上实测 | |
|---|---|---|
| **0 个对比度失败** | **`contrastFails: 0`** | ✓ |
| **最差 4.63** | **`worst: 4.63`** | ✓ **逐位** |
| **阶梯 `[16,14,13,12,11]`** | **`ladder: [16,14,13,12,11]`** | ✓ **逐位** |
| **`costBeforeClick` true** | **`requests: 4` · `encodedBytes: 39,279` · `htmlBytes: 9,386`** | ✓ |
| **而那个按钮** | **`buttons: []`** ✗ | **✗ 主语在别处** |

**而 `costBeforeClick` 那一行还有一个**更值钱的细节** ✓✓**：**`beforeFcp` 说两个 script 在
**805 ms / 847 ms** 开始 ✓，**而 FCP 是 **864 ms**** ✓——**即**那个 wasm 的 fetch
**不在** FCP 之前**✓✓**——**而它正是 P4 的设计 ✓**（**"构建时用 Rust 渲染 ✓ · 文字进 HTML ✓ ·
**事后 hydrate** ✓"**）——**而那是**第一次**有数字证明它 ✓✓**（**之前只有 `hasWasm: false` ✓**）。

**防法**：**一条判据要写清它的**主语**✓✓**——**"按钮 rgb(176,58,10)/20px/492×40/白字" ✗
**与"**console 登录页**的按钮 rgb(176,58,10)/20px/492×40/白字" ✓ **是两条不同的判据 ✓**——
**而一个读起来像"这个页"的句子 ✗，会让下一个人**量错地方，然后报一个不存在的失败**✗✓。


### P1 的普查：**19 个门禁，搬 8 · 留 10 · 删 0**（2026-09-28 ✓）

**而 P1 说**"普查（留/删）→ 先搬真的在解析东西的" ✓。**而这一节就是那次普查 ✓**——
**判据是三个问题 ＋ 三个判决 ✓，**而每一个判决都带着它的测量 ✓。**

**判据** ✓✓：**① 那个不变式（一句话 ✓）· ② 它是**唯一**检查它的吗 ✓（**判据是一个 `grep` ✓**）·
③ `MUTATION:` 块在不在 ✓。**

**搬 8**（**全部开工了 ✓**）——**而这一列是 2026-09-29 实测的进度 ✓，判据是那个 `.mjs` 还在不在**：
| 门禁 | 现状（`ls scripts/test/<名>.mjs`） | 不变式 | 而它的事故 |
|---|---|---|---|
| **`workflow-yaml-check`** | **搬了 ✓ → `agent/tests/workflow_yaml.rs`**（`b3230b0c`，合入 `446a1f01`） | 每个 workflow 都能被 YAML 解析 | **35 个提交 · 0 个 job · `conclusion: failure` · 无日志可开** |
| **`chrome-stillness`** | **搬了 ✓ → `agent/tests/chrome_stillness.rs`**（`c799c34d`，合入 `bccfb8a9`） | 每条动画必须申报为 STATE / ENTRANCE(≤400ms) / ATTENTION(≤5s) | **一个装饰性脉冲只对**要求了的人**静音** |
| **`console-marks`** | **还在 ✗** | 5 个 mark 家族：每状态一个轮廓 **且** 两主题墨色 ≥3:1 | **`--text-faint` 在浅色下 **2.46:1**** |
| **`feedback`** | **还在 ✗** | 三张表：`:hover` 必有 `:active` · transition 不动布局 · ≤240ms · 有 reduced-motion 块 | **74 个 `:hover` 对 3 个 `:active`** |
| **`motion`** | **还在 ✗**（**最小的一个 ✓：123 行，两张表，一条 `prefers-reduced-motion` 规则 ✓**） | 每条动画必须在 reduced-motion 里**按名字**被静音 | **`--ds-dur: 0s` 够不到它 4 条动画中的 **0** 条** |
| **`particles`** | **还在 ✗** | 三份粒子场（含落地页 **Rust**）只用品牌 token / `rgba()` 三元组 / 尊重 reduced-motion / 参数一致 | **退役的 `--aura-*` 调色板**在画整个产品的背景 |
| **`token-contract`** | **还在 ✗**（**最大的一个 ✗：514 行 ✓**） | 两端都定义的同名 token 必须解析到同值、无死回退、间距刻度一致 | **16 个同名 token 有 12 个不同值** |
| **`docs-budget`** | **搬了 ✓ → `agent/tests/docs_budget.rs`**（`7bf53b7c`，合入 `40c48cbe`） | `AGENTS.md` ≤48,000 B · `CONTEXT.md` ≤12,000 B · 7 个操作章节在 | **`AGENTS.md` 到 65,366 B，撞上 65,536 的截断** |

**而三条搬完的，每条都留下了一个数 ✓**：`docs-budget` 是**九个输入两版同判 ✓**（含 48,000 整与 48,001
超一字节 ✓）· `chrome-stillness` 是**四个树上的用例 ＋ 三个 fixture 探针，正文逐字节相同 ✓**
（120 / 332 / 280 / 476 与 2083 / 1574 / 1867 ✓）· `workflow-yaml` 是**全history 335 个 revision-file
对，两版同判、同拒那 3 个 ✓**——**而它把 Node 那条路**去掉了**✓**：`serde_yaml` 在进程内 ✓，
于是**那个「没有 PyYAML 时的兜底」和 `--differential` 开关一起消失了 ✓✓**。

**而 `chrome-stillness` 那一条是**从「哪里都没到」捞回来的 ✓✓**：那 690 行**写完了却没提交 ✗**
（`git log --all -- '*chrome_stillness*'` 答**空** ✓），**而它连 `cargo clippy` 都过不去 ✗**
（7 个 error ✓：三个 `map_or` ＋ 四个 doc 缩进 ✓）——**即它离落地只差一次 clippy ✓，
而没人跑它，因为没人有它 ✓**。

**而地板跟着走，且**规则化**了 ✓**：三次搬迁各自手改过一次地板 ✗，于是它现在写的是**规则**✓——
**地板 = 推导出的条数 − 4 ✓**（4 是 round 170 那个「留一次有意的删除、不留一次塌方」的余量 ✓），
**每次照同一条推导重量一遍 ✓**：32 → 31 → 30 → 29，地板 26 → 26 → 25。

**而同一夜抓到一条**只在这个环境里红**的 ✓✗**——**记下来，因为它是**下一条要查的**✓，
而它现在**没有解释 ✗**：`3024bc5e` 那次 CI 的 `pack-chain` 里，**`all-gates` 的第 24 步
（「every gate, in one command」）在 `cargo test -p summrise-agent --features terminal,keyring`
上红 ✓**：**`756 passed; 1 failed` · `error: test failed, to rerun pass '-p summrise-agent --lib'`** ✓。

**三条测量，而它们指向**同一个方向 ✓**：① **同一个提交的 `agent` job 跑了**同一条命令**，
**绿 ✓**；② **本机跑同一条命令：`757 passed; 0 failed`，exit 0 ✓**（69.24s ✓）；
③ **`all-gates` 只打印**尾部 ✗**——**那个测试的**名字不在日志里**✗✓，
**所以「哪个测试」这一步**必须下次在 CI 里重跑才能拿到**✓**（`--no-fail-fast` ＋ 不截断输出 ✓）。

**而它为什么值得单独记** ✓：**`pack-chain` 是**没有 Rust 缓存**的那个 job ✓**——
**它编译整棵 crate 是冷的 ✓**，而**同一个命令在 `agent` job 里是热的 ✓**——
**即这条红的形状是「冷环境下有一个 lib 测试会红」✓**，**而那不是 flake 的同义词 ✗**：
**一个只在冷编译/慢机器上红的测试，通常是一个**真的时间/端口依赖**✓**（本仓库为此付过
`ci-not-in-flight` 与 `main-shape-shallow` 两次 ✓）。**下一次不要重跑等它变绿 ✓——先拿到名字 ✓。**

**留 10**，**每一个都有它自己的理由** ✓：**`console-assets` ✓（**驱动 `npm run build` ✓**）·
`console-smoke` ✓（**跑 Node 脚本，要构建产物 ✓**）· `contrast-probe` ✓（**AGENTS.md 豁免：
注入页面的代码 ✓**）· `landing` ✓（**输入是 JS 渲染的产物 ✓**）· `main-shape` ✓（**"最弱的一个" ✓**）·
`npm-test-floored` ✓（**它就是那一步 ✓**）· `panel-sheet-freshness` ✓（**驱动构建 ✓**）·
`press-anchor` ✓（**豁免：注入代码 ✓**）· `state-colour` ✓（**一个 Rust 版必须**复制**那个注入的函数 ✓**）·
`workflow-shell` ✓（**权威就是 bash ✓，Rust 重写只能是近似 ✓**）**

**删 0** ✓✓——**而它**认真尝试过杀掉四个最像冗余的 ✓，**全部失败 ✓**：
**两个 assets/sheet 新鲜度门禁各管一面 ✓ · `main-shape` 的 hook 看不见 fast-forward ✓ ·
两个 workflow 门禁问的是不同问题 ✓ · 而 `landing-check` 的安装命令**无人替代**✓。**
**而且删有**硬预算**✓✓**：**`all-gates` 用运行器自己的推导得到 **32** 条 vs floor **30** ✓**——
**"删到第 3 个，运行器就直接拒跑（this proves nothing）" ✓✓**。

**而真实的 `MUTATION:` 比例** ✗✓：**11/18 有（61% ✓）· 7 个没有 ✓**——
**而其中 6 个连 `SELF_TEST|fixture|planted` 都是 0 ✓**——
**即 `AGENTS.md` 那句**"每个门禁头部都带自己的变异证明"**对这 7 个是**假的**** ✗✓**。
**而**每一个被搬的 Rust 版**都是**第一个**为它那个门禁带证明的 ✓✓**——
**即**门禁**不只是被搬 ✓——**它们**在被搬的过程中**变完整了 ✓✓。**
