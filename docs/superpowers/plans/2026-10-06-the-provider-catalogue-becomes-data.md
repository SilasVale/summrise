# 提供商目录变成数据 · 计划（2026-10-06）

> **操作者的原话**（2026-10-06，两轮）：*"这些不能参考 deepseek harness 第三方模型提供商，自定义模型，那些一模一样设计吗，
> 因为现在就 opencode go 的套餐在使用，其他的我们都没有用呢，所以这里面好多都可以去掉，模型提供商感觉可以使用 deepseek
> harness 的设计，但是用 rust 语言"* · *"为什么 goal 目标里面又要求写 js 呢，不合理吧，不是要全部迁移到 rust 吗"*
>
> 这份文件是那两句话的可执行版本。**它取代的是"变差就留 JS 版 / 门禁做不到等价就留着 .mjs"那两条**——那两条读起来像
> "继续写 JS"，而操作者要的是相反的东西。

---

## 一、今天的实测（每个数旁边是产生它的命令）

| 事实 | 数 | 命令 |
|---|---|---|
| Rust 行数 / 文件 | **114,808 / 217** | `git ls-files \| … wc -l`（见下表脚注） |
| JS 家族合计（.ts/.tsx/.mjs/.js/.cjs） | **~153,800 / 571** | 同上 |
| 内置渠道前缀 | **9**（`og/ or/ nv/ gmi/ qw/ cm/ r4/` + `ds/ amd/`） | `gateway/src/channels.ts` 的 `ROUTE_INFO` 与 `ROUTE_TABLE` |
| 健康卡 | **21** | `HEALTH_CHANNELS` 的 `{ id:` 计数 |
| **实际在用的渠道** | **1**（`og/` = opencode go，操作者 2026-10-06 明确） | 操作者原话 |
| 控制台 Models 页 / 存储 / wasm 逻辑 | `Models.tsx` **1,127** · `store/models.ts` **345** · `store/providers.ts` **542** · `wasm/store.rs` **492** · `wasm/registry.rs` **655** · `ui-logic/lib.rs` **725** | `wc -l` |
| 切流后可从 `gateway/src` 删掉的 `/v1` 半边 | **~4,407 行**（translate / translate-vision / upstream / channels / reliability / tokens / anthropic-translate） | `wc -l` 那七个文件 |
| `gateway/src` 里**不是** `/v1` 的部分（控制台 API＋插件＋store） | **~9,650 行** | `gateway/src` 总 14,057 减去上一行 |

### JS 家族按目录（行数，`git ls-files` + `wc -l`）

| 目录 | 行数 | 判定 |
|---|---|---|
| `agent/resources/panel-react` | 42,500 | **留**：浏览器 UI（TS/TSX）；逻辑已在 `panel-logic/`（Rust） |
| `gateway/test` | 20,251 | **随旧半边消失**：`/v1` 的用例在切流、等价证明完成后删 |
| `gateway/public` | 19,031 | **生成物**：Source Viewer 的镜像，随 `agent/scripts` 走 |
| `gateway/src` | 14,057 | **切流后删 `/v1` 半边**（~4,407）；控制台半边见 A6 |
| `agent/summrise-agent-npm` | 13,741 | **留**：npm 的交付方式就是 JS（⑦） |
| `gateway/ui` | 9,695 | **留**：浏览器 UI；逻辑已在 `ui-logic/`（Rust） |
| `agent/scripts` | 8,909 | **可搬**：sweep 的驱动与判据（A5，待点头） |
| `agent/summrise-desktop-electron` | 4,207 | **留**：⑥ 操作者已定不迁 |
| `proxies/api-relay` | 3,916 | **一半已 Rust**（`relay/` crate）；JS 半边是 oracle，切流后删 |
| `gateway/wasm` | 3,773 | **harness**：`verify.mjs`/`route-oracle.mjs` 必须执行 JS+wasm 产物（见"例外"） |
| `agent/summrise-workspace-dsh` | 2,707 | **留**：DSH 插件包，插件必须是 JS |
| `index/*`、`proxies/zen-*`、`relay/test`、`index/spike-*` | ~8,000 | 旧半边/oracle，各自的切流完成后删 |

---

## 二、B：提供商目录变成数据（形状照 DSH，实现用 Rust）

### 今天的形状 vs 目标形状

DSH 的 provider 记录（本机 `~/.dsh/profiles/*/cordis.patch.yml`，逐字读过）：

```yaml
providers:
  <name>:
    apiKeyEnv: VALE_API_KEY      # 密钥从哪个环境变量读
    api: openai-completions      # 协议（另有 openai-responses 等）
    baseURL: https://…           # 上游根
    models:
      - id: og/mimo-v2.5
        name: …
        contextWindow: 1000000
        maxTokens: 128000
```

Summrise 今天有 `providers:custom`（`store/providers.ts`），记录是
`{ prefix, label, models[{ id, name, contextWindow, maxTokens, input }] }` —— **缺 `api`、`baseURL`、`apiKeyEnv` 三个字段**，
所以一个自定义提供商的上游仍由 `upstream.ts` 的内置表决定（`resolveRoute` 的三段：内置表 → 自定义 → 默认出口）。

**B 就是把那三个字段补上，让一个 provider 自描述**；然后内置表里没人用的部分可以整段删除，因为留下来的项就是这张表的第一条记录。

### 落地顺序（每条一个分支、一组语料）

1. **Rust 侧类型与读取**：`gateway/wasm/src/store.rs` 增加 provider 记录的读写（KV 键沿用 `providers:custom`，
   记录升级为含 `api`/`baseURL`/`apiKeyEnv` 的形状；旧记录缺字段时按内置表回退，一个提交）。
2. **路由自描述**：`routing.rs` 的 `resolve_model` 在自定义提供商分支上使用记录里的 `baseURL`/`api`；
   `bearer_key_for` 使用 `apiKeyEnv`。差分语料：同一模型的**上游 URL、方法、Bearer、正文**，与今天逐字节相同。
3. **控制台**：`Models.tsx` 的提供商表单补齐三个字段，读写的仍是同一条记录；`ui-logic`（Rust）里加校验。
4. **删死目录**：**每删一个渠道，先量它没人用**——
   - KV：有没有 `ukeys:*` 记录带该渠道的键（`wrangler kv key list` / 控制台）；
   - 日志：`journalctl -u vrelay` 与 worker 的访问日志里该前缀的出现次数；
   - 操作者确认一次。
   然后一个家族一个提交：渠道 → 它的健康卡 → 它的 BYOK 键位 → 它的测试与语料。
5. **判据**（每条都能失败）：
   - `og/` 的每条路由/翻译/流式用例逐字节等价（现有 `verify.mjs` + 11 份语料 + `front-door-cors-corpus.json`）；
   - `/v1/models` 对**在用**模型的输出与今天逐字节相同（`models-corpus.json` 重生成后比较）；
   - `HEALTH_CHANNELS` 从 21 张卡降到在用集合，且 `/api/health` 的字节与新表一致；
   - 门禁全绿，`cargo test -p summrise-gate-wasm` 的用例数只增不减。

---

## 三、A：迁移收尾

- **A1 切流①**（gateway 流式路由 → wasm）：前置已在本会话补齐并实测——前门顺序、两种 token 拼写、env 输入、
  BYOK 记录形状、每请求 CORS；流式响应与发货路由 **3/3 逐字节相同**。剩下是部署动作（zone route、回滚、操作者点头）。
- **A2 切流②**（relay 的 axum 二进制上 VPS）：本机跑通、六条路由 curl 过；只卡在盒子 `132.226.90.175` 的
  `authorized_keys` 一行。
- **A3 切流后删除** `gateway/src` 的 `/v1` 半边（~4,407 行）——**先证明等价，再删**。
- **A4** 上表每一块的判定写进提交信息；"留"的每一块都要有一句"为什么它必须是 JS"。
- **A5（待点头）** `agent/scripts` 的 sweep 驱动与判据（~8,909 行 .mjs）→ Rust。成本：一个构建阶段 + Node↔Rust 边界；
  `agent/src/plugins/design/` 已是在产品里做同一件事的 Rust 先例。
- **A6（待点头，第二阶段）** 控制台 `/api/*` 与插件（~9,650 行 TS）→ wasm：同一个 worker、同一个目标，理论可搬；
  先量收益（冷启动、体积）再决定。

---

## 四、例外（必须留 JS 的，逐条写清为什么）

| 面 | 为什么不能搬 |
|---|---|
| `agent/resources/panel-react/`、`gateway/ui/` | 浏览器要执行它；**逻辑已经搬进 Rust**（`panel-logic/`、`ui-logic/`），留下的是渲染 |
| `agent/summrise-agent-npm/` | npm 的交付方式就是 JS（⑦，操作者已定） |
| `agent/summrise-desktop-electron/` | Electron 壳（⑥，操作者已定：兼容性） |
| `agent/summrise-workspace-dsh/` | DSH 插件包——插件必须是 JS，这不是本仓库的选择 |
| `gateway/wasm/verify.mjs`、`route-oracle.mjs` | 它们**执行 JS+wasm 产物**并驱动发货 TypeScript 做差分；产物是 JS，harness 只能在 Node 里跑它。切流删掉旧 TS 后，`route-oracle.mjs` 随之退役 |

---

## 五、要操作者点头的三件事

1. **A5**：sweep 驱动/判据搬去 Rust（成本已列）。
2. **A6**：控制台 `/api/*` 搬去 wasm（第二阶段）。
3. **B4 的删除清单**：确认"只留 `og/`"，其余 8 个前缀与 21 张卡里用不到的部分按上面的顺序删。
