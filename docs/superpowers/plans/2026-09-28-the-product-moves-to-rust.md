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

### P1 的普查：**一个门禁能不能搬，由它 spawn 什么决定**（2026-09-29 ✓）

**计划要的"普查（留/删）"就是这个 ✓——而它在此之前**从没被写下来**✓，而它的答案**不是行数**✓✓。**

**判据是环境，不是代码**：`cargo test` 跑在 **`agent` job** 里 ✓，而那个 job **只有 cargo**——**没有 `npm ci` ✓、
没有 node_modules ✓、没有浏览器 ✓**（`ci.yml` 里 `ui` job 是 `npm ci && npm run build` ✓，`design` job 才装 playwright ✓）。
**所以一个门禁要搬进 `cargo test`，它必须**不 spawn 任何需要依赖的东西**✓**；否则只有两条路 ✗：
在 `agent` job 里加装依赖 ✓（**真金白银的 CI 成本 ✓**），或者**在缺依赖时静默跳过** ✗✗——
**而"静默跳过"正是这份计划自己列的**门禁悄悄豁免**✗。**

**实测（`grep -oE 'exec[A-Za-z]*Sync\("[a-z]+"' scripts/test/*.mjs` ✓）：**

| 门禁 | 它 spawn 什么 | 能不能搬 |
|---|---|---|
| `token-contract-check` ✓ | **无**（纯读两个 CSS ＋ 算对比度 ✓） | **能** ✓ |
| `feedback-check` ✓ | **无**（读 CSS ＋ 源码 ✓） | **能** ✓ |
| `contrast-probe-check` ✓ | **无** | **能** ✓ |
| `state-colour-check` ✓ | **无** | **能** ✓ |
| `landing-check` ✓ | **无** | **能** ✓ |
| `main-shape-check` ✓ | `git` ✓（agent job 里有 git ✓） | **能** ✓ |
| `console-assets-check` ✗ | `git` ＋ **`npm`**（它**跑一次 console 构建** ✓） | **不能** ✗ |
| `npm-test-floored` ✗ | **`npm`** | **不能** ✗ |
| `panel-sheet-freshness-check` ✗ | `git` ＋ **`npm`** | **不能** ✗ |
| `console-smoke-check` ✗ | **`node`**（jsdom 渲染冒烟 ✓） | **不能** ✗ |
| `press-anchor-check` ✗ | **`node`**（playwright ✓） | **不能** ✗ |

**所以 P1 的**可搬集合是 6 个**（约 1,558 行 ✓），而**不是"所有剩下的"**✓✓——**而已经搬完的 11 个
（`docs-budget` ✓、`workflow-yaml` ✓、`chrome-stillness` ✓、`motion` ✓、`particles` ✓、`console-marks` ✓、
`workflow-shell` ✓ …）**全都在这个集合的形状里**✓：**它们读文件、算，然后说话 ✓。**

**而剩下那 5 个不是"还没做"✓，是**具名的例外**✓✓**——**它们要的不是一次移植，而是一个决定**✓：
**要么承认它们永远留在 `.mjs`** ✓（**而那与 AGENTS.md 里那四条 carve-out 是同一类东西：有理由的、被点名的** ✓），
**要么给 `agent` job 加 `npm ci` ＋ 一个浏览器** ✓——**而那会让每一个 Rust 提交都多付一次装依赖的钱 ✓**，
**为了 5 个门禁 ✓。这个决定属于操作者 ✓。**

### 而普查还有**两条约束**，而它们是**读出来的、不是假设的**（2026-09-29 ✓）

**上一节的表只问了"它 spawn 什么"✓。而这一轮要动 `contrast-probe-check` 和 `main-shape-check` 时，
两条各自不同的约束把两个都挡下来了 ✓——而两条都不是 grep 能看出来的 ✓。**

**约束 ②：**门禁的**主体**必须是**数据**，不能是它**执行**的 JS** ✓✓。
`contrast-probe-check` **import 了 `contrast-probe.mjs` 然后调用它** ✓：
`assert.equal(contrastRatio({r:162,g:163,b:172}, bg), 5.49)` ✓。
**它钉的是**那个 JS 模块的数学**✓——**而把它翻成 Rust 就是钉 Rust 的数学 ✓，那**是另一个门禁**✓✓**
（**而"另一个门禁"在这里不是小事：探针跑在浏览器里，而那个模块是浏览器真正执行的那份 ✓**）。
**判据因此要精确到**主体还是工具**✓**：`token-contract-check` 也 import 了 `contrastRatio` ✓，
**但它的**主体是两套 CSS token**✓，对比度只是**工具**✓——**工具可以重写 ✓（`console_marks.rs` 里已经有一份 ✓），主体不能 ✗**。
按这条：`state-colour-check` 过 ✓（`loudnessOf` 是工具 ✓），**`landing-check` 不过** ✗
（**它的主体就是 `index/src/page.js` 里那些字 ✓，而"把 JS 当文本读"是一个**更弱的检查**✓**）。

**约束 ③：**门禁的**证明**不能在**别人的仓库**里运行它** ✓✓。
`main-shape-check` 的证明是 `main-shape-shallow.bash` ✓：它**真的 `git clone --depth 1`** 到一个临时目录 ✓，
`cd` 进去，然后 `node "$GATE"` ✓——**因为要复现的正是**浅克隆的 graft**✓，而那是别的方法造不出来的 ✓。
**而 `cargo test` 永远是**对着自己那个 crate 的仓库**跑的 ✓——**所以这个门禁**不能是 cargo test**✓✓**，
**除非把它的证明也一起改掉 ✓——**而那正是这个门禁存在的理由 ✓**。

**所以可搬集合是 3 个，不是 6 个**✓✓：

| 门禁 | 行数 | 为什么能 |
|---|---|---|
| `token-contract-check` ✓ | 514 | 主体是两套 CSS ✓，对比度是工具 ✓ |
| `feedback-check` ✓ | 301 | 主体是三张样式表 ✓ |
| `state-colour-check` ✓ | 150 | 主体是 CSS 的颜色 ✓，`loudnessOf` 是工具 ✓ |

**而三条约束合起来是一句话 ✓**：**一个门禁能搬进 `cargo test`，当且仅当它读**数据**、不 spawn 依赖、且它的证明**就在本仓库里**** ✓✓。

**AND ONE MORE THING THIS ROUND FOUND, WHICH THE PLAN WARNED ABOUT AND HAD NOT NAMED** ✓：
**`main-shape-check.mjs` 根本没有 `MUTATION:` 块** ✓（`grep -c MUTATION` = 0 ✓）。
**计划那句"先读那个门禁的头——不要假设它有 `MUTATION:` 块（至少有一个根本没有）"✓，那个"一个"就是它 ✓。**
**而它并不是没有证明 ✓——它的证明是那张**六行 fixture 表** ✓（`SELF_TEST` ✓，含那对"`main` ＋ 1 个父提交必须拒 ✓、
`change/x` ＋ 1 个父提交必须过 ✓"✓）——**只是那张表在**正文里** ✓，不在头部的 `MUTATION:`/`RESULT:` 形状里 ✓。**

### `token-contract` 是**八个检查装在一个文件里**，而这就是它还没搬的原因（2026-09-29 ✓）

**它是最后一个可搬的门禁 ✓（514 行 ✓，其中 391 行真代码 ✓），而这一轮**读完了它、没有动它**✓✓**——
**因为把它照原样翻成 Rust 大约 900 行 ✓，而"读完了"和"翻完了"是两件事 ✓。**
**而这一节把它从一个"大文件"变成**八个有名字的块**✓，因为那才是让下一轮能开工的东西 ✓。**

| # | 那个检查 | 它读什么 | 它输出什么 |
|---|---|---|---|
| ① | `divergences` ✓ | 两边的 `:root` / dark 块 ✓ | 共有名字里**值不同**的 ✓（比的是**含义** ✓，空白先压掉 ✓） |
| ② | `offScaleNeutrals` ✓ | console 的 `--bg/--border/--text/--chrome*` ✓ | **不在 panel 那张尺子上**的中性色 ✓（19 个全在 ✓） |
| ③ | `deadFallbacks` ✓ | 两棵源码树里每个 `var(--x, v)` ✓ | **系统声明过的 token 上的回退** ✓（panel 曾有 38 个 ✓，console 0 个 ✓） |
| ④ | 语义色的**文字权重** ✓ | console 的两个主题块 ✓ | `--success-text/--warning-text/--error-text` 必须两边都声明 ✓；`color: var(--text-muted)` 这类**拿标记权重当文字**的 ✓ |
| ⑤ | **强调色家族** ✓ | 两个主题的 `--accent`/`--accent-fg`/`--bg` ✓ | AA 4.5 的两个方向 ✓（白配 `#d9480f` 4.30 ✓、`#d9480f` 当文字 4.12 ✓、深色主题登录键 1.90 ✓） |
| ⑥ | **落地页**的共有名字 ✓ | console 与 landing 的**有效集** ✓（`:root` ＋ 覆盖 ✓） | 不重合的 ✓——**而它用**具名覆盖**而不是计数 ✓**（`--glass-blur` ✓、`--ds-font-family` ✓ 必须在交集里 ✓） |
| ⑦ | **间距尺** ✓ | 两边的 `--sp-0-5 … --sp-5` ✓ | 少一个 ✓、或值不同 ✓（**共有名比较看不见的那一半** ✓：一侧删掉一个步进，它就不再"共有" ✓） |
| ⑧ | 落地页的**可解析性** ✓ | `index/landing/assets/page.css` ✓ | 没有 `:root` 就**致命** ✓（一个指向空串的解析器会报"干净" ✓） |

**而 ③ 和 ⑦ 各自带一个**为什么必须单独存在**的理由 ✓✓，两条都是踩出来的：**
**③ 的失败消息曾经把死回退说成"两个表面有 N 个 token 意思不同"✓**——**它把读者送去找一个不存在的值不匹配 ✓**；
**⑦ 存活于 ① 之上** ✓——**① 只看两边都定义的名字 ✓，所以一侧**删掉**一个步进时它静默退出 ✓**，
**而那正是"尺子悄悄变成五级"的那一半 ✓。**

**AND ONE THING WORTH CARRYING INTO THE PORT** ✓：**那个文件里有一个 `let failures = 0` 被放在它第一次使用**之后** ✓**，
**而它的注释记着后果 ✓**：干净的树永远走不到那一行、检查通过 ✓，**而**有**死回退的树不是报错、是 `ReferenceError` 崩掉 ✓✓**——
**"一个只在自己有话要说时才坏掉的门禁，是最糟的形状"** ✓，**而退出码恰好非零，正是让 CI 看不见它的原因 ✓。**

**AND IT MOVED ON 2026-09-30 ✓ —— `agent/tests/token_contract.rs` ✓，`.mjs` 已删 ✓。** **八个检查一个不少 ✓**，
**而等价证明是十一个植入用例 ✓**：每个检查一次 ✓（分歧 ✓、离尺中性色 ✓、死回退 ✓、语义色的文字权重 ✓、
强调色家族 ✓、落地页共有名 ✓、间距值 ✓、**间距步进被删** ✓、落地页可解析性 ✓），
**外加两个"不该咬"的对照 ✓**（只有一侧定义的 token ✓、在未声明 token 上的回退 ✓）——
**两版在每一行输出与每一个判决上不可区分 ✓✓。** **而移植抓到的三处引擎语义 ✓**：
`Math.round` 半数向 +∞ ✓、`Math.max(0, NaN)` 是 NaN ✓（`f64::max` 会返回另一个操作数 ✓）、
**以及 `Object.keys` 是插入序 ✓**——用 `BTreeMap` 会让每条多 token 消息的**顺序**都不同 ✓，
**而判决全部一致 ✓**：一个看起来像噪声、其实是另一个程序的偏差 ✓。
**那个 TDZ 的故事也搬过去了 ✓**：Rust 版**边跑边打印 ✓**，因为 JS 就是那样 ✓——
**第一版把行收集起来最后打印 ✓，被差分抓到 ✓**：落地页 `:root` 断言中途失败时 ✓，
**前面的检查说过的话全丢了 ✓。**

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
| **`token-contract`** | **搬了 ✓ → `agent/tests/token_contract.rs`（2026-09-30 ✓，`.mjs` 已删 ✓）** | 两端都定义的同名 token 必须解析到同值、无死回退、间距刻度一致 | **16 个同名 token 有 12 个不同值** |
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


### P2 的**第三条路**：预加载，而它比惰性**快** 12.6 ms（2026-09-29 ✓）

**计划把渲染路径的价钱算成了两条路 ✓**：**① 内联字节**（+18,913 gz ✗）与 **② 挪到数据边界**（0 gz ✓，
**但带三个前提** ✓）。**而这两条都建立在一个前提上，而那个前提**从没被量过**✓：
**"wasm 在第一次调用时取"** ✓——**而它是 P2 一开始自己选的（criterion ③）✓，不是环境给的 ✓。**

**第三条路是**：**在 `index.html` 里用一个 inline module 取并编译，`main.tsx` 在第一次渲染前 await 它** ✓✓。
**而它的成本是**负的** ✓✓：

| | 第一次出现 `#root` 的子节点（ms，n=10，交替 A/B，同一个浏览器） |
|---|---|
| 惰性（第一次调用时取） | **median 141.4** · mean 142.4 |
| inline + 渲染前 await | **median 128.6** · mean 126.7 |
| **配对差（inline − 惰性）** | **median −12.6** · mean −15.6 · **10 对里 10 对为负** ✓✓ |

**为什么更快，而这不是直觉的答案 ✓**：**贵的是**编译**，不是取** ✓。**`<link rel="preload">`
把取提前到同一个时刻 ✓，**但把 `WebAssembly.compileStreaming` 留给胶水 ✓——即留给 `main.tsx`
运行的那一刻 ✓，而那里它和 React 的第一次渲染抢主线程 ✓**：**实测 +11 ms**（各 11 次，
median 133.8 → 145.0 ✗）。**inline module 在 panel.js 还在下载时就编译完 ✓**（bundle 比模块大 ~11× ✓）。
**两个变体的 wasm 请求数都是 1 ✓✓**（预加载被复用，没有取两遍）。

**而那个数是 `#root` 的第一个子节点，不是 FCP ✓✓**：`main.tsx` 在 React 挂载前就起了装饰性粒子场 ✓，
**所以 FCP 是那块 canvas ✓**——**实测 FCP 88 ms 而模块还在飞 ✓**。**一个量错地方的仪器会
把两个版本报成一样 ✓，而那正是防错第 6 条的形状 ✓。**

**而它解掉的是**三个前提**本身 ✓✓**：**`disambiguateLabels` 是计划自己点名的"明确不过"的那个 ✓**
（**它的输入是**调用点选的子集**✓：`TabBar` 数 `sessions` ✓、`DesktopShell` 数 `openTabs` ✓，
**一次边界派生不可能同时是两者 ✓**）——**它现在是**第九个家族** ✓，**而两个条带各自传自己那份列表 ✓，
语义与 TS 版逐字节相同（31 个语料，见提交信息）✓✓。**

**代价（已量、已记）** ✓：**首屏载荷 +816 gz**（272,344 → 273,160 ✓），**其中 +482 gz 是
`index.html` 那一个 inline module 与它的注释 ✓——一次性 ✓**；wasm 本体 +1,790 gz（**它不在首屏载荷里 ✓**）。

**而它同时废掉了 12 个文件里的一句**现在已经不成立的话**✓**（"the wasm is fetched at the first call" ✓）——
**逐个改成历史时态并指向 seam ✓**，因为**一个读者相信的、没人再核过的前提，就是这一夜一直在付的那种账 ✓。**


### P3 的**目标运行时**不是三种，而是一种**加一种**（2026-09-29 实测 ✓）

**而这一条是"测量本身是错的"那一条，犯在计划自己身上 ✓**：P3 写的是"**三个 Worker**：
`gateway/src/` · `index/` · `proxies/`"，**而 `proxies/` 里只有两个是 Cloudflare Worker** ✓✓。

| 目录 | 是什么 | 证据（不是推断） |
|---|---|---|
| `proxies/zen-go-proxy/` | **Cloudflare Worker**（`opencode-go-proxy`）✓ | `wrangler.jsonc` ✓ · 403 行 ✓ |
| `proxies/zen-us-proxy/` | **Cloudflare Worker** ✓ | `wrangler.jsonc`（zone route）✓ · 339 行 ✓ |
| `proxies/api-relay/` | **VPS 上的 Node 进程**（vrelay @ Oracle）✗ | `build.sh` 的 `deploy_api_relay` 是 `scp` + `ssh` ✓ · 仓库自己的 `proxies/README.md:48` 写着"**it ships to the VPS, not to Cloudflare**" ✓ |
| `proxies/summrise-relay/` | **自己机器上跑的 Node 进程** ✗ | 目录里**没有 `wrangler.jsonc`** ✓ · `node relay.mjs --listen 127.0.0.1:18990` ✓ |

**所以 P3 的"全部搬成 wasm"这句话，对一半的代码是错的** ✓：**`api-relay` 就是本仓库自己的 git 远端**
（就是本仓库的 `origin` 指向的那个 `/api/git` 路径，AGENTS.md 记着 ✓✓）——**而它要变成的是一个 Rust 二进制跑在盒子上，
不是一个 Workers 模块** ✗。**两套目标运行时，两套构建，两种证明** ✓：
workers-rs ＋ `worker-build`（`gateway/wasm` 已经把这条路走通了 ✓）对 `zen-*` / `index` / `gateway` ✓，
而 `api-relay` / `summrise-relay` 是 **axum/hyper ＋ 一个静态链接的二进制**，**它的"smoke"是
`proxies/api-relay/api/test/*.test.mjs`** ✓。

**而 P3 的顺序（`proxies` → `index` → `gateway`）本身仍然成立** ✓——**因为它选的是爆炸半径，不是运行时** ✓。
**而搬第一个之前要先说的是搬哪个运行时** ✗：**`zen-us-proxy` 一周 1 个请求** ✓（P3.1 实测），
**`api-relay` 是本仓库自己的 push 路径** ✓——**把 git 远端换成一个没部署过的 Rust 二进制，
是这个计划里风险最高的一步** ✗，**它值得第一个做，也值得最小心地做** ✓。


### P3 的第一块：`/api/version` 的**清单推导**（2026-09-29 ✓，38 个语料全等）

**为什么是这条路由 ✓**：**它是每个设备的 updater 都会调的那条** ✓，**也是六个路由里唯一"答案里没有
I/O"的一条** ✓——**它读 `version.json`（已经取到了）、校验、把每一个 URL 重建到本次请求的 origin 上** ✓。
**而"字节可比"这件事，只有在没有 I/O 的那一半上才问得出来** ✓✓：**ASSETS 取、R2 对象、GitHub 代理
要比较，必须有一个能同时跑两边的运行时** ✗，**而这台机器的 `wrangler dev` 起不来**（GLIBC 2.31 ✗，P3.0 已记）。

**所以做法是**：**把 I/O 留在外面，推导做成纯函数 `(origin, version_json, warn) -> 答案`** ✓。
**差分把同一份 `version.json` 和同一个 origin 喂给两边，比三样东西**：**状态码、正文字节、
`console.warn` 的每一行** ✓✓。**38 个语料，0 处不同** ✓。

**而"警告也在被比较的表面里"不是讲究 ✓**：一个坏的 pin 被**丢掉并且说出来** ✓，**而"不说"才是
round 134 找到的那个缺陷** ✓——**只比正文的话，这个移植会带着一句悄悄错的警告通过** ✗。

**差分抓到四处，两处是我们自己一开始就错的** ✓：
① `String(val.url).split("/").pop()` 取的是**最后一段** ✓——**这正是"一个完整 URL 在这里可以安全接受"
的全部理由** ✓，**第一版把整个 URL 当成名字，于是每个 component 都被丢掉，清单悄悄不带任何 pin** ✗
（**正是 round 134 那个失败，被一个更短的函数重新引入了一遍** ✓）；② **component 的 key 也要 JSON 转义** ✓
（**带引号的 key 是合法 JSON 输入** ✓，**原样输出得到的是 `{"a"b":…}`——不是 JSON，发给每个设备的 updater** ✗）；
③ **正文用 `push_str` 拼，不用一个大 `write!` 加 `{{}}`** ✓——**raw string 不处理 `{{` 转义** ✓，
第一版发出的是 `{{"version":…}` ✗；④ **警告里的括号是"被拒 digest 的前十二个字符"** ✓。

**而这个 crate 一开始谁也不跑** ✓✓：`index/worker/` 在 agent 的 workspace 之外，**agent job 的
`cargo test` 看不见一个不属于它的 crate** ✗。**这正是本仓库 `chrome_stillness` 烂掉的方式** ✓
（690 行、写完、提交、**从来没编译过**，七个 clippy 错误没人看见 ✓）。**所以它是现有 `index` job 里的
一个 STEP，不是新 job** ✓，**而 `all-gates.bash` 从那行 `run:` 推导它**，本地也跑（31 条，多了一条 ✓）。

**而 `all-gates` 自己也得学一个路径 ✓✓**：它原来把每条 cargo 都 `cd agent` ✓，于是
`--manifest-path index/worker/…` 从那里跑会答"could not read Cargo.toml" ✓——**对的地方跑了错的命令**，
**看起来像一个红的 crate，而它什么也不是** ✓✓。


### ⑤ 的 URL 边界也在 Rust 了，**而 `index/src/index.js` 还在 JS**——**这是依赖图强制的顺序** ✓

**`page.js` 的三个导出**（`safePageUrl` · `escHtml` · `PAGE`）现在是 `index/worker/src/lib.rs` ✓，
**644 个语料、0 处不同** ✓（132 个白名单 × 4 个 fallback ✓ · 17 个转义 ✓ · 495 次**拿真实的
`setup.js` / `npm-only.html` 渲染整页** ✓✓）。

**而 TypeScript 没有被删 ✓**，因为 `index/src/index.js` 还在 import `PAGE` ✓——**这就是计划里
"旧的在新版被证明等价之前不删" 的用法** ✓：**同一段规则在两种语言里存在一个步骤** ✓，
**而唯一让这可接受的东西，是它们在一个语料上被证明相等，而不是仅仅并存** ✓✓。
**它们会在删掉那个 import 的同一个提交里一起消失** ✓。

**两份文档是参数，不是 embed ✓✓**：`page(arm, …)` 收**文本**，**而这正是产品将来的用法**
（worker embed `index/landing` 在构建时生成的两份）✓。**在这里 embed 它们是一个构建顺序问题**——
**一个陈旧的 `include_str!` 会发布一个陈旧的落地页** ✓——**所以在 worker 真的去读它们之前，这件事
刻意不做** ✓。

**差分抓到的那一处，是这一类里最贵的一种** ✓：**`Option<&str>` 不是 JS 的 `null`** ✗——
**TypeScript 测的是真值（truthiness），`Option` 测的是存在** ✓。**于是 `Some("")` 选了 setup 那一份**，
**给一个没有发布安装包的 release 发了一个带"下载 Windows 安装包"按钮的页面** ✗✗——
**正是 round 125 那个缺陷，被一个 `Option` 重新引入了一遍** ✓，**而对着 Rust 写的每一个单元测试都看不见它** ✓✓。
**同一个 harness 还在两轮之前抓到过一次同一类**（空的 tarball 名）✓。

**而这一轮我还改了两个自己的测试期望，因为它们对 JavaScript 的理解是错的** ✓：
**`""` 是被接受的** ✓（`new URL("", base)` 就是 base，协议是 https ✓），
**`http://LOCALHOST` 是 loopback** ✓（hostname 会被转小写 ✓）——
**差分才是权威，一个把代码"修"成符合错误期望的测试才是那个 bug** ✓✓。


### ④ 的**路由表**（决定的那一半）也在 Rust，**而两边的 I/O 都还没搬** ✓，52 个路径 0 处不同 ✓

`index/worker/src/routes.rs` 回答"这个请求是哪一条路由" ✓，**而每条路由的 I/O 一行都没动** ✓
（两个 ASSETS、两个 R2、一个 GitHub 代理、落地页渲染 ✓）——**和清单同一形状**：**能比字节的只有决定 ✓**。

**语料是"差一点就命中"的路径 ✓**：多一个 `.bak` ✓、少一段版本号 ✓、`v1.2.297` ✓、四段版本号 ✓、
`01.2.297` ✓、**大写的 `.EXE` / `.TGZ`** ✓、尾部斜杠 ✓、`..` 穿越 ✓、**一个 Unicode 连字** ✓——
**因为路由表错的地方都在那里** ✓，**而"缺的二进制必须 404、绝不能把落地页当 200 HTML 返回"
（设备真的下载过 HTML 当安装包，然后 agent 起不来 ✓）这条规则就是为它们写的** ✓✓。

**而这个差分的第一版有两个"分歧"，**两个都是我的仪器错的** ✓✓：
**① 分类器用 `endsWith(" version.json")` 找清单读取，而真实字符串是 `ASSETS /…/version.json`** ✓；
**② 两边吃的是不同的输入**——**worker 走 `new URL(request.url).pathname`，而空路径在那里被归一化成 `/`** ✓，
**我把原始请求路径喂给了一边、把归一化后的喂给了另一边** ✗。**一个错的测量看起来和一个缺陷一模一样** ✓✓
（**而这正是防错第 6 条** ✓）。**第一版更糟的是用正则把 `if` 链从文件里"提"出来** ✗——
**它编不出 JavaScript**，**因为一个改动了它所测试的东西的仪器，测的是别的东西** ✓。
**现在 JS 那一侧跑的是发货的那份文件本身**，路由由**谁回答了**来认 ✓✓。


### ④ 的 CDN worker **能构建了**：I/O 层 · 两份文档 · 一个只为 wasm32 存在的入口 ✓

`worker-build --release` 产出 **`index.js` 28 KB ＋ `index_bg.wasm` 552 KB** ✓✓，而它只 import 两样东西：
**`cloudflare:workers` 和它自己的 wasm** ✓——**正是 gateway 那个 harness 已经打桩的那两样** ✓。

**入口是 wasm32-ONLY 的，而这**不是**可移植性妥协 ✓✓**：`#[event(fetch)]` 在 host 上**展开成空并 panic**
（那里没有 Worker 运行时可注册）✗。**所以 `src/worker.rs` 是 `#[cfg(target_arch = "wasm32")]`** ✓，
**而三个纯函数、全部测试、两个差分在 host 上照常构建和运行，一个 wasm 工具链都不需要** ✓✓。
**另一条路是给 host 写个 shim**——**那等于多一份没有任何测试跑过的 dispatch 副本** ✗，
**而这正是本仓库吃过亏的形状** ✓。

**两份文档是 `include_str!` 进来的，不是抄一份** ✓：`index/landing/build.sh` 生成
`index/src/landing/{setup,npm-only}.js` ✓，**JS worker import 同样的两个文件** ✓，
**外加一个 `build.rs` 在任一个变化时重跑 cargo** ✓✓。**另一条路是一份可以被提交成陈旧版本的
生成 `.rs`** ✗——**那正是 `contract.gen.ts` 和它的门禁存在的原因** ✓——**而一个陈旧的落地页比一个
陈旧的生成文件更糟** ✓。**而那条测试又纠正了我一次**：第一版断言 setup 那份含
`SummriseAgent-Setup.exe` ✓，**它不含** ✗——**门是那个 SLOT** ✓，**那个字符串只出现在白名单的兜底里** ✓。

**已证 / 未证，分两张单子写清楚** ✓：
**已按字节对上游 JS 证过**：清单推导（38）✓ · 页面边界（644，含 495 次**拿真实文档**的整页渲染）✓ ·
路由表（52，含全部近似路径）✓。
**已构建**：模块只 import 那两样 ✓。
**尚未**：**这个模块还没有被执行过** ✗。**`verify.mjs` 是下一步，而它需要的绑定现在是量出来的而不是猜的** ✓✓：
`EnvBinding::get` 按构造函数名 duck-type ✓，**所以 env 桩就是 `class Fetcher` 和 `class R2Bucket`**
（worker-0.8.7 `env.rs:148` ✓）——**gateway 自己的 `verify.mjs` 已经是这么给 Durable Object 打桩的** ✓。

**而编译器替我抓到的三个 API 错误值得记下来** ✓：`resp.ok()` 在这个版本里是**关联函数**
（该用 `status_code()`）✗；`Headers::get` 答的是 `Option<String>`，**不是带 `to_str` 的 HeaderValue** ✗；
**`Bucket::get()` 返回的是 builder 不是 Future** ✗——**该调 `.execute().await`** ✓，
**而第一版 await 的是那个 builder** ✗。**三个都是凭记忆猜的 API，而凭记忆猜的 API 是一次迁移会反复犯的一类错** ✓✓。
**`Fetch::Request(req).send()` 才是那个全局 fetch** ✓——**`Fetcher` 是给别的 worker 的 handler 用的，
而且根本没有公共构造器** ✗。


### ④ 的 CDN worker **跑起来了**：13 个用例，逐字节对**发货的那份 JS** ✓✓

`index/worker/verify.mjs` 在 Node 里执行 `build/index.js`（`worker-build` 的产物 ✓），和
`index/src/index.js` **跑同一个请求**，比**状态码、排序后的 header、正文字节** ✓。
**"已构建"变成"已执行"** ✓✓。

| 用例 | 结果 |
|---|---|
| 清单（有无安装包）· 200 | 173 / 342 B **逐字节相同** ✓ |
| 清单（缺失 / digest 不可验）· 503 | 83 B **逐字节相同** ✓ |
| 落地页两种 arm × 两个路径 · 200 | 27,316 / 27,421 B **逐字节相同** ✓ |
| tarball 与安装包别名 · 200 | 11 B **逐字节相同** ✓ |
| electron 运行时与 playwright 包 · 200 | 8 B **逐字节相同** ✓ |
| 不指向任何产物的路径 · 版本号差一点 · 404 | 9 B **逐字节相同** ✓ |
| cloudflared 代理 | 决定 ✓ ＋ pin ✓ |

**而第一跑就抓到四个真缺陷** ✓✓——**这正是"跑它"的意义** ✓：
① **错误体不是 JSON** ✗：JS 的 helper 是 `JSON.stringify({ error: \`${what}: ${detail}\` })`——
**一个字符串，用冒号连起来** ✓，而第一版拼出了 `{"error":"…":"…"}` ✗——**正好差一个字节，且无法解析** ✗，
**而这是设备 updater 每一次都会轮询的那条路由的失败体** ✗✗。
② **404 声称自己是二进制** ✗：JS 从字符串体推出 `text/plain;charset=UTF-8` ✓，而那一版走的是产物字节路径、
**答了 `application/octet-stream`** ✗；**用一个空 header map 去修，结果把类型整个弄没了** ✗（下一跑报的）✓。
③ **playwright 那条带 `content-length`，electron 那条不带** ✓✓——**JS 本身不对称** ✓，
**差分正好在两条里抓到那一条** ✓：**这就是"两条都加吗"的答案——不加，而对齐要读源码，不是读原则** ✓。

**而仪器自己在给出答案之前，形状错了三次** ✓✓：R2 桩的 `get` 答了 builder 而不是 **promise**
（"e.then is not a function"）✗；对象的 `size`/`http_etag` 写成了**方法**而 workers-rs 读的是**属性** ✗；
以及一个普通对象而 `ObjectInner::Body` 包的是 `web_sys::Response` ✗。**中间那一次产出了这个提交历史上
最吓人的输出**：一个 **58 字节的"产物"，内容是某个 JavaScript 函数的源码文本** ✗✗——
**而那正是设备会写进磁盘的东西** ✗。**桩必须是绑定期待的形状，不是读代码那一方调用的形状** ✓✓。

**loader 是从 `gateway/wasm` 注册的，不是抄一份** ✓——**一个 loader，两个 worker** ✓✓。
**旧的一侧是按文件 import 的** ✓：**`data:` URL 装不下 `index.js`**，因为它 import `./page.js` ✓，
**而 data: URL 没有层级可以解析相对说明符** ✗。**第一版那么做了，失败是模块解析错误，看起来却像 worker 的错** ✗
——**和路由差分第一版同一种形状，同一个答案** ✓✓。

**变异**：改 `src/worker.rs` 里 503 的 detail 文本并重新构建 → **恰好 2 个用例红** ✓（那两条失败路由）✓，
**另外十一条照样绿** ✓✓——**失败是关于一条路由的发现，不是一个坏了的仪器** ✓。

产物：`index.js` 24,964 B（6,528 gz）＋ `index_bg.wasm` 565,504 B（185,004 gz）✓。


### ④ 的 CDN worker **已经部署到一个 canary 名** ✓，而 **canary 本身在这两台机器上都够不着** ✓

`index/wrangler.rust.jsonc`：`name: summrise-dist-rust` · `main: worker/build/index.js` ·
**同一份 `./public` 资产 ＋ 同一个 `summrise-temp-files` R2 桶** ✓✓。
`wrangler deploy` 的输出就是**部署证明了什么**的记录 ✓：

    Uploaded 14 of 14 assets   （2 already uploaded）
    Worker Startup Time: 3 ms
    env.TEMP_FILES (summrise-temp-files)   R2 Bucket
    env.ASSETS                             Assets
    https://summrise-dist-rust.zhengsaisi.workers.dev
    Current Version ID: d66001a6-c71b-4278-aba0-7416b2a85c46

**所以：模块被接受 ✓、资产按生产同一目录上传 ✓、两个绑定都绑上 ✓、启动 3 ms ✓。**

**而"运行时答什么"还没量 ✓，因为 `*.workers.dev` 在这个循环用到的两台机器上都够不着** ✓✓：

| 机器 | canary | 生产 |
|---|---|---|
| **这台 Linux 开发机** | `TLSv1.3 handshake … Connection reset by peer` ✗ | 200 ✓ |
| **设备 d1（OpenWrt）** | `CANARY 000 0` ✗ | **同一个 shell 同一秒 200** ✓ |

**这和 `wrangler tail` 是同一条网络边界** ✓（`scripts/build.sh` 早就记着 tail 是 TLS-reset ✓），
**而两台机器上生产都是 200，所以它不是部署的错** ✓。

**于是"还差什么"是可指名的 ✓**：一个真的 `R2Object` 的 `size`/`httpEtag` 作为**属性**返回 ✓，
以及 ASSETS 直通**保留 range 与条件头**（`verify.mjs` 的桩从没见过它们）✓。
**这是切流前最后一个未知项 ✓，而它需要一个能看见 workers.dev 的网络** ✓✓——
**或者一条挂在一个可达域名上的 zone route ✓，那是一个带 DNS 后果的决定，因此这里不取** ✗。

**而 canary 留着不删 ✓**，理由是**删除本身是第二个不可逆动作** ✗，**而留着能让一个网络可达的人立刻
smoke 它** ✓✓——**它带着版本 ID 写在上面的注释里** ✓。


### ④ 的 CDN worker **smoke 过了**，而它找到了一个**任何 harness 都看不见的缺陷** ✓✓

canary 挂上了 zone route `agent-rust.saisi.online` ＋ 一条 CNAME（**两条都可删**：删记录 ＋ 删 route）。
**八条路由对着 `agent.saisi.online` 本身逐字节相同**（把主机名归一化之后）✓✓：
落地页两个路径 ✓ · 清单 ✓ · 404 ✓ · tarball ✓ · cloudflared ✓ · **两个 R2 产物，都拿满全尺寸** ✓。
**主机名是唯一的差别，而这正是 worker 本来就该有的差别** ✓——**每个 URL 都按本次请求的 origin 重建** ✓，
**所以清单不可能把设备指向另一个 host** ✓。

### 而那个缺陷：`ObjectBody::stream()` **把 R2 对象穿过 isolate 拉一遍** ✗

`ObjectBody::stream()` **把对象读进 isolate** ✓；`ObjectBody::response_body()` **把原始流直接交给运行时** ✓
——**而 JS 做的正是后者** ✓（`new Response(obj.body, …)`）✓。

**在 115 MB 的 electron 运行时上量到的** ✓✓：

| | 结果 |
|---|---|
| `stream()` | **每一次都不满** ✗：21.6 / 71.8 / 76.5 / 80.6 MB（共 115,028,145） |
| `response_body()` | **三次都拿满** ✓：34 / 50 / 54 s |
| 生产自己 | 52.5 s ✓ |

**而 32 MB 的 playwright 对象两种写法都传得完** ✓✓——**所以 13 个 verify 用例、8 字节的桩、
以及所有本地测量，在"115 MB 下载悄悄坏掉"的同时全绿** ✗✗。**这就是先部署到一个没人用来装东西的
名字上、再动那个名字的理由** ✓✓。

**而"控制组"这一手也是必要的**：**生产在这条链路上也会截断** ✓（一次只到 18.3 MB ✓），
**所以"慢就断"是这个环境的性质** ✓，**而 canary 三次都拿满、生产三次都拿满** ✓——
**差别是速度，不是正确性** ✓。


### ④ 切流了：**`index` 的生产域名现在由 Rust worker 服务** ✓✓

`index/wrangler.jsonc` 的 `main` 指向 `worker/build/index.js` ✓——**这是整个迁移里的第一次切流** ✓，
而且是**每一个设备的 `setup` 都用的那个域名** ✗✗。

**而 `src/index.js` 没有删** ✓✓：`index/test/*.mjs` 还在 import 它 ✓，
**而"旧的在新版被证明等价之前不删"在"证明等价"有两个来源之后才真正被考验** ✓：
① `verify.mjs` 13 个用例对着**发货的那份 JS** 逐字节 ✓；
② **canary 用同一份资产、同一个 R2 桶，对着这个域名，八条路由逐字节** ✓✓（主机名归一化后）。
**回去的路是一行** ✓：`"main": "src/index.js"` ＋ 重新部署 ✓。

**而构建是部署的一部分** ✓：**`worker-build` 的输出整个是 gitignored 的** ✓，
**所以没跑过它的 checkout 根本没有 `main` 可部署** ✗，**而 wrangler 的报错是一个路径错误，
一个字都不提构建** ✗。**`deploy_worker` 现在先跑 `worker-build --release` ✓，跑不出来就拒绝部署** ✓，
**并把 `cargo install worker-build` 那行打出来** ✓——**因为缺工具和缺模块看起来一模一样** ✓。

**切流后在真机（生产域名）上量到的** ✓✓：

    wrangler deploy exit 0 · version a5c0d508-8f7f-4d7d-b107-993a0d73c4f8
    deployment 5028711b · Worker Startup Time: 2 ms · 两个绑定都绑上
    post-deploy smoke: 「no installer advertised for v1.2.495, and the SummriseAgent-Setup.exe alias is
                      absent (consistent)」←✓ **round 125 那条规则被既有门禁当场验了** ✓
    post-deploy smoke: 「/api/version smoke passed (v1.2.495, versioned + latest binary sha verified)」
    **那条曾经坏掉的路由：115,028,145 字节，两次** ✓✓（82.7 s / 59.2 s，生产自己早先是 52.5 s）

**而落地页和清单的字节和切换前一样** ✓——**这正是"已证明等价"该有的意思** ✓。

**所以 block ④ 的 `index` 这一块完成了** ✓✓：**纯函数逐字节证过 ✓ · 能构建 ✓ · 跑起来对拍过 ✓ ·
canary 对着生产逐字节过 ✓ · 切流了 ✓ · 真机上量过了 ✓。**


### ④ 的第二个 worker `zen-us-proxy`：**origin 策略与 secret 脱敏在 Rust 了**，261 个用例逐字节 ✓

`proxies/zen-us-proxy/worker/src/lib.rs` ✓：**34 行里能搬的部分全搬了** ✓——
`isLoopbackOrigin` ✓ · `isLoopbackHost` ✓ · `requestHost` ✓ · `corsHeaders` ✓ ·
**`redactSecrets`（最难的那个）** ✓ · `jsonError` 的 body ✓。
**碰网络、碰 request、碰流的那些一行没动** ✓✓。

**三个事实，三个坑** ✓：
① **`redactSecrets` 按长度降序排，而这个顺序就是全部** ✓——**一个 secret 若是另一个的子串，
先长的被遮住** ✓；**升序会把长的尾巴留下来，变成 `*******cret` 而不是 `***`** ✓✓。
另外：**先去重** ✓ · **短于 8 个字符的忽略** ✓ · **非字符串也忽略**（**这是个真过滤不是类型断言** ✓）。
② **`isLoopbackOrigin` 解析，`isLoopbackHost` 比较** ✓——**两个不同的问题** ✓，
**而 CORS 规则两个都要** ✓：**loopback 的 `Origin` 只有在请求自己的 host 也是 loopback 时才被反射** ✓，
**所以部署出去的代理绝不会反射一个外部页面的 `http://localhost`** ✓✓。
③ **而 `isLoopbackHost` 和落地页那个 loopback 规则** ***不是同一个事实*** ✗：
落地页是 `Some("localhost") | Some("127.0.0.1") | Some("[::1]")` ✓（round 449 刻意加的）✓，
**这里只有两个 hostname，没有 `[::1]`** ✓✓。**两份拷贝已经漂移了，而这次移植保留各自的答案** ✓——
**统一它们是对一个 CORS 决定的改动，不是重构，而这里没有任何证据说明哪一边是有意的** ✓。
**所以它被记下来** ✓：**两个 worker、两套 loopback 规则、其中一套没有理由** ✓——**这是诚实的状态** ✓。

**而差分的语料专门压在上面三件事上** ✓：**重叠的 secret** ✓ · **长度的两侧（7 与 8）** ✓ ·
**重复的 secret** ✓ · **非字符串** ✓ · **多行与 Unicode 文本** ✓ · **会弄坏手写转义器的字符**
（引号、反斜杠、换行、tab、控制字符）✓。**261 个用例，0 处不同** ✓✓。

**而"非字符串"这一类恰恰证明了这个差分存在的理由** ✓：`&[&str]` **让这个输入无法表达** ✗，
**所以一个 Rust 单元测试只能"证明"它——办法是把数字当字符串传进去** ✗，**那是一个不同的输入，
而且会断言出相反的结论** ✓✓。**这一类在差分里证，因为只有那里输入真的能是非字符串** ✓。

**而这轮的仪器自己错了五次，每一次都不是代码的错** ✓✓——**值得逐条记下来** ✓：
① **抽取函数时从第一个 `{` 开始数花括号** ✗，**结果数到了 `jsonError(…, cors = {})` 的默认值上** ✗
**返回了半个函数** ✗，**而 `new Function` 报的错误在五行之外** ✗；
② **只把 JS 一侧排序、拿插入序的 Rust 去比** ✗，**47 处"不同"全是同一条目不同顺序** ✗；
③ **把 `LOCALHOST` 直接喂给 `corsHeaders`** ✗，**而 JS 那一侧是过 `new URL` 的（小写化）** ✗
——**10 处"不同"全部来自仪器给一边 header 值、给另一边解析后的 hostname** ✓✓
**而这一对函数是"成对"的**：`cors_headers` 收的是 hostname，**唯一能产出 hostname 的就是 `request_host`** ✓，
**类型系统拦不住一个传原始 header 的调用者** ✓；
④ **驱动里把 status 写死成 400** ✗，**于是 5 处"不同"都是"Rust 答了 400"** ✗；
⑤ **`argv` 传不了 NUL 字节** ✗，**那个用例到了 JS、没到 Rust，而仪器把它算成"不同"** ✗——
**一个传不动的字节是仪器的问题，文件才是传得动它的 transport** ✓✓。


### ④ 的第三个 worker `zen-go-proxy`：**协议翻译与 SSE 编码在 Rust 了**，294 个用例逐字节 ✓

`proxies/zen-go-proxy/worker/src/lib.rs` ✓：`toOpenAIRequest` ✓ · `toAnthropicResponse` ✓ ·
**`sse` ＋ `toSSE`** ✓。**而 origin 策略那五个函数没有重打一遍** ✓——**它们在 zen-us 的 JavaScript 里
逐字节相同** ✓，**所以这个 crate 依赖那个 crate** ✓：**一种规则在一种语言里写两遍，
正是两个卫星已经开始漂移的形状** ✓✓（**就是 `[::1]` 那一处** ✓）。
**ADR 0003 让卫星 worker 保持自治（不共享 npm 包）** ✓，**而一个共享的 Rust crate 不是共享部署** ✓。

**而 `serde_json` 的 `preserve_order` 不是可选项** ✓：**默认的 `Map` 是 `BTreeMap`，按键排序** ✗，
**所以移植出来的对象会以另一个顺序出现** ✗——**而这个响应是按字节比的** ✗。

### 这一轮真正的东西：**"undefined 是一个被 JSON.stringify 丢掉的键"** ✓✓

**这一条在差分里出现了 FIVE 次** ✓：消息的 `id` ✓ · fallback 分支的 `role` 与 `content` ✓ ·
一个 tool 的 `name` ✓ · 一个 `tool_use` 块的 `id` ✓ · `tool_call_id` ＋ `tool_choice` 的 `name` ✓。
**第一版在每一处都写了 `unwrap_or(Value::Null)`** ✗，**于是没有名字的 tool 带着 `"name":null` 上去了** ✗，
**没有 id 的 completion 带着 `"id":null` 回到了期望"要么有键要么没有"的客户端那里** ✗。
**`null` 是调用者发来的一个值，而"字段缺失"是值的缺席** ✓，**在线路上这两者不可互换** ✓✓。

**而另外三处是同一个"别当得更宽容"的家族** ✓✓：
① **一个 `messages` 字符串按它的字符迭代** ✓——**`for (const m of req.messages || [])` 迭代任何可迭代对象** ✓，
**所以十二个字符就是十二条消息** ✓，每条 `{}` ✓；**第一版用 `as_array()` 给出的是空对话** ✗，
**而空对话是 provider 会接受的** ✗✗——**那是危险的方向** ✓。
② **一个 falsy 的 `content` 不是抛错，是空数组** ✓✓（`m.content || []` ✓）——
**所以第一版在 `content: null` 上 panic 是错的** ✗，**而那个用例差分语料里有** ✓。
③ **一个分数的 token 数是原样透传的** ✓：**`prompt_tokens: 1.5` 进了 `1.5`** ✓，
**而返回 `u64` 的第一版给出的是 `0`** ✗——**那是丢失，不是四舍五入** ✗。

**而三处 `TypeError` 变成了 panic** ✓✓：**content 里的 `null` 块** ✓ · **数字的 `content`** ✓ ·
**`null` 的 tool** ✓——**三个都是差分"崩掉"发现的，不是"报出不同"发现的** ✗✓，
**而一个崩掉的仪器告诉你有东西，一个报出不同的仪器告诉你是什么** ✓。
**两边都拒绝就是一个等价 ✓**——**15 个用例正是这样，而把它们算成失败会推动一个移植去"不再拒绝畸形输入"** ✗，
**那恰恰是它唯一不能做的事** ✓✓。


### ④ 的 `zen-us-proxy` **入口的决定部分也在 Rust 了**，282 个用例逐字节 ✓

路由表 ✓ · 两个门（`x-api-key` 与 `Bearer`）✓ · **session 头的四头优先序** ✓ · **`safeEq`** ✓。
**上游取数与流式中继没动** ✓✓。

**路由表里三条与直觉相反的规则** ✓：
① **`OPTIONS` 是自己的答案且排在最前** ✓✓——**预检永远到不了门，也永远不需要凭证** ✓。
② **两条门控路由是 `endsWith` 而不是相等** ✓✓——**所以挂在前缀下部署仍然工作** ✓。
③ **其余一切是 404，而 404 在读 key 之前就到** ✓：**默认关闭关了两次** ✓✓，
**已知的路径但没配 `CLIENT_KEY` 也被拒绝，而不是落到付费 key 上** ✓。

**而 `x-api-key` 门是"三重默认关闭"** ✓：**没配 secret 拒绝** ✓ · **配成空串也拒绝** ✓✓
（**`!env.CLIENT_KEY` 是真值测试，`Some("")` 用 `== ""` 去拦就漏了** ✓）·
**两种失败答同一个 401** ✓，**所以调用方分不出"没配"和"配错了"** ✓✓。
**而 `safeEq` 两条规则都是时序规则** ✓：**没有长度早退** ✓ · **32 字节全折叠不短路** ✓，
**一个写成 `if a[i] != b[i] { return false }` 的移植是一个正确但漏信息的函数** ✗。

**`Bearer` 前缀大小写敏感，slice 固定七** ✓✓：`bearer x` **不是** Bearer 头 ✓，
`Bearer  x` 剩下一个前导空格由 trim 去掉 ✓，**而恰好 `Bearer ` 什么也不剩，所以是拒绝** ✓。
**这条路由上没有 `CLIENT_KEY` 而且是刻意的** ✓：**调用方带自己的 key，这个 worker 一次也不替换** ✓。

**session 头是一个顺序，而这个顺序就是特性** ✓：**第一个非空胜出** ✓，
**所以一个带全四个头的请求转发第一个、丢掉其余三个** ✓，
**而空格会被 trim 掉因此当作不存在** ✓✓。

**而这一轮仪器错了六次，其中一次最值得记** ✓✓：
**我的 `safeEq` 覆盖版忘了 `new Uint8Array`** ✗——**`digest` 答的是 ArrayBuffer，
而 `buffer[i]` 是 `undefined`** ✗，**于是 `undefined ^ undefined` 是 0，折叠是 0，
`safeEq` 对任意两个字符串都答 true** ✗✗。**一个永远说"是"的仪器，和一个宽松的实现是无法区分的** ✓，
**而 28 个用例看起来像"对的门错了"** ✗——**因为"差异的方向"是读者最先信、最后该信的东西** ✓✓。
**worker 自己那行是 `const a8 = new Uint8Array(da)`，而覆盖版现在有它** ✓。

## 收口普查（2026-09-30 ✓）：**可搬的逻辑搬完了，剩下三件是操作者的**

按计划自己的判据逐项对过一遍，每条都带能跑的命令：

| 块 | 状态 | 证据 |
|---|---|---|
| **P0** | ✓ 完成 | `docs/superpowers/p0/`：187 个 LOGIC 导出、基线、wasm-pack 证明 |
| **P1** | ✓ 可搬的搬完 | `agent/tests/*.rs` **42 个门禁**；`scripts/test/*.mjs` 只剩 **7 个具名例外** |
| **P2 面板** | ✓ 收口 | `panel-logic/` **29 个模块**；`lib/`+`hooks/` 里**在 crate 里或具名 boundary**。三样数（2026-10-03 实测 ✓）：**wasm 84,216 gz**（`agent/resources/panel/panel_logic_bg.wasm`，`index.html` **解析时预加载**所以算首屏 ✓）· **胶水 14,746 gz**（`panel-react/src/wasm/panel_logic.js`，**被打进 panel.js** ✓）· **首屏 ~359,731 gz**（panel.js 190,415 + panel.css 84,400 + wasm 84,216 + index.html ~700） |
| **P2 控制台** | ✓ 收口 | `gateway/ui-logic/`；五个文件在 crate，三个是**具名决定**（`keyNames`/`i18n`/`api`）。三样数（2026-10-03 实测 ✓）：**wasm 15,779 gz** · **胶水 6,792 gz** · **首屏 ~130,678 gz**（`gateway/public/` 的 js 106,438 + css 7,090 + wasm 15,779 + html 1,371） |
| **P3 Worker** | ✓ 逻辑搬完 | `gateway/wasm`（**20 个模块，70 测试，11 份语料**）· `index/worker`（20）· `zen-us`（11）· `zen-go`（17） |
| **P3 VPS** | ✓ 决策搬完 | `proxies/api-relay/relay/`：**27 个测试、9 份语料**——`api-relay` 的五个 handler 与 `summrise-relay` 的 token/路由 |
| **P4** | ✗ 不做（默认） | 已量：4.7× 体积换 85 行 JS |

**而操作者 2026-09-30 的三个决定** ✓：

1. **P4 做** ✓ —— 而**它已经落地了** ✓：`index/landing/` 在构建时渲染整份文档 ✓，
   `index/src/landing/{setup,npm-only}.js` 是它产出的两个 arm ✓，
   而 `index/src/page.js` 只剩三个 URL 的替换与策略 ✓。
   **这一轮补的是接线** ✓：`build.sh index` 现在会重新渲染 ✓，
   而 `index` 作业跑 `cargo test --manifest-path index/landing/Cargo.toml` ✓——
   **那条测试把"跟踪的 arm 就是这个 crate 渲染出来的"钉住** ✓（变异：改 `page.rs` 一个字而不重新生成 →
   失败，并打印"Regenerate with `index/landing/build.sh`" ✓）。
2. **P1 那 8 个例外逐个复查** ✓——**1 个搬了、1 个只搬了一半** ✓（**这一行 2026-10-03 按实测改正** ✓）：
   `landing-check` **已搬完** ✓（它不 spawn 任何东西 ✓）；
   `contrast-probe-check` **只搬了 13 条断言** ✓（`agent/tests/contrast_probe.rs` ✓），
   **剩下的 5 条是关于 `PROBE_SOURCE` 这个 JS 产物的** ✓——而计划的 carve-out **明确保留它**
   （`browser_run_script` 收一个 JS 文件 ✓），所以那 5 条是**具名例外**而不是待办 ✓。
   **原表写的"2 个能搬"是把整个文件当成了主语** ✓，而它自己的头文件早就写着为什么不是 ✓。
   **6 个不能** ✓：`console-assets-check` / `panel-sheet-freshness-check`（跑 npm 构建 ✓）、
   `console-smoke-check`（跑 smoke 脚本 ✓）、`main-shape-check`（问 git ✓）、
   `npm-test-floored`（跑 npm test ✓）、`press-anchor-check`（跑发射器 `--emit` ✓）。
3. **P3 切流：待操作者确认对 relay 的理解** ✓（它在回答里问"relay 是什么" ✓）。

**而剩下的是两件计划从一开始就写明"由操作者决定"的事** ✓（**2026-10-03 按实测改正**：第三件 P4
**操作者已经点名"做"** ✓，而它**已经落地** ✓——上面第 1 条自己写着这件事，所以这一段原先在自相矛盾 ✓）：

1. **P3 的切流**：gateway 的流式路由改走 wasm ✓、relay 的 axum 二进制上 VPS ✓——
   **都是部署动作**（要凭据、要挑时间、要能回滚）✓。计划对 `api-relay` 的警告仍然成立：
   **它是本仓库自己的 push 路径** ✓，**把一个没部署过的二进制换成 git 远端之前，
   先让它跑起来、再让它当远端** ✓。
2. **P1 那 8 个 `.mjs` 的 CI 依赖**：计划写着"加不加 CI 依赖由操作者决定" ✓。
3. ~~**P4**：只有操作者点名才做~~ ✓ **已点名、已落地** ✓——`index/landing/` 在构建时渲染整份文档 ✓，
   而 `index` 作业的 `cargo test --manifest-path index/landing/Cargo.toml` 把"跟踪的 arm 就是这个 crate
   渲染出来的"钉住 ✓。

**三样数的诚实限定** ✓：**"胶水对首屏的增量"那一栏量的是胶水自己的 gz** ✓，**不是实测增量** ✗——
它被打进 `panel.js`/`index-*.js` ✓，而**反事实（去掉 wasm 的构建）没有量** ✗。要把它变成实测增量，
就得构建一次没有 wasm 的面板与控制台 ✓——**那是一次独立的测量** ✓，写在这里而不是含糊过去 ✓。

**所以这一份计划的可执行部分到此为止** ✓——**再往下走需要上面三个决定中的任意一个** ✓。
