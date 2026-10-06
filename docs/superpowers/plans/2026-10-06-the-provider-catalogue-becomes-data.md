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

**这一行原先写错了，2026-10-06 按实测改正**：Summrise 的 `providers:custom`（`store/providers.ts`）**已经有** DSH 的
那几个字段——记录是 `{ prefix, label, baseURL, api, apiKeyEnv?, apiKey?, models[{ id, name, vision?, contextWindow,
maxTokens, reasoningEffort? }] }`，而 `upstream.ts` 的 `resolveRoute` 本来就是三段：内置表 → 自定义 → 默认出口。

**真正缺的是 Rust 侧的那一半**：`gateway/wasm/src/store.rs` 只搬了**列表**那一半（`advertised_provider_models`，
所以 `/v1/models` 会广告 `acme/acme-chat`），而 `routing.rs` 的 `resolve_model` **只查内置表**。所以一个自定义提供商的
模型会被广告、然后被拨到默认渠道——**正是 `providerRoute` 自己的注释拒绝的那种失败**（"AN UNROUTABLE RECORD IS AN
ERROR ROUTE, NEVER THE DEFAULT CHANNEL … it would dial a built-in upstream under a different provider's name"）。

实测（2026-10-06，构建产物 + 同一份 KV，两侧都 stub 掉 `fetch` 抓请求）：

```text
    发货:  POST https://acme.test/v1/chat/completions   Bearer sk-acme-inline
    wasm:  502 config_error — "CMD_API_KEY not configured — add your Command Code key"，NO CALL
```

**所以 B 的第一步不是"补三个字段"，而是把这个分支搬过去**（已完成，见 A 段的记录）；补字段是第二步，因为记录本身
已经够自描述了。

### 落地顺序（每条一个分支、一组语料）

1. ~~**Rust 侧读取与路由**~~ **已完成（2026-10-06）**：`store.rs` 的 `provider_for_prefix`/`provider_key`/
   `provider_model_vision`/`provider_key_env_names`，`routing.rs` 的 `is_built_in_prefix`/`provider_route`/
   `resolve_model` 三分支，`v1.rs` 读 `providers:custom` 并把每个记录的 `apiKeyEnv` 绑定读进 env。
   语料：`provider-corpus.json`（11 例，发货链）与 `provider-request-corpus.json`（3 例，抓到的上游请求）。
   **实测过的坑**：`apiKeyEnv` 是记录自选的**任意绑定名**，固定名单读不到它——`verify.mjs` 的
   "a NAMED binding's value rides Bearer" 抓到了这一版自己的 bug。
2. **控制台**：`Models.tsx` 的提供商表单与 `ui-logic`（Rust）对齐这条记录；今天的表单已经是同一形状，只差核对。
3. **删死目录**：**每删一个渠道，先量它没人用**——
   - KV：有没有 `ukeys:*` 记录带该渠道的键（`wrangler kv key list` / 控制台）；
   - 日志：`journalctl -u vrelay` 与 worker 的访问日志里该前缀的出现次数；
   - 操作者确认一次。
   然后一个家族一个提交：渠道 → 它的健康卡 → 它的 BYOK 键位 → 它的测试与语料。
4. **判据**（每条都能失败）：
   - `og/` 的每条路由/翻译/流式用例逐字节等价（现有 `verify.mjs` + 语料 + `front-door-cors-corpus.json`）；
   - `/v1/models` 对**在用**模型的输出与今天逐字节相同（`models-corpus.json` 重生成后比较）；
   - `HEALTH_CHANNELS` 从 21 张卡降到在用集合，且 `/api/health` 的字节与新表一致；
   - 门禁全绿，`cargo test -p summrise-gate-wasm` 的用例数只增不减。

---

## 三、A：迁移收尾

- **A1 切流①**（gateway 流式路由 → wasm）：前置已在本会话补齐并实测——前门顺序、两种 token 拼写、env 输入、
  BYOK 记录形状、每请求 CORS；流式响应与发货路由 **3/3 逐字节相同**。剩下是部署动作（zone route、回滚、操作者点头）。
  **切流的判据现在是一条数**：`verify.mjs` 的**分歧扫描**（同一份 KV/env/stub 上游/记录型 BreakerDO，发货前门 vs 构建
  产物，逐案比状态、正文、头、抓到的上游请求与 DO 调用序列）。2026-10-06 首跑 **11/18**，7 处不同归成四个家族——
  熔断的读侧（`channelDegradedError`）、写侧（`/trip` 与 `/reset`）、按 arm 分的失败信封、每 token 限流；四个都补完后
  **29/29 相同、0 处已知差异（2026-10-06）**。**"还要多久"就是这条数到 0 的距离**，不是日期——而这条数现在到 0 了：
  剩下的只有部署动作（zone route、回滚、操作者点头）。最后关掉的是**视觉预处理**（`translate-vision.ts` 313 行：
  `VISION_MODEL` 描述图片、块换成文字、用户级 SHA-256 KV 缓存 7 天、十个后端表、失败必须失败请求）。
  **切流的确切形状（2026-10-06 实测，可回滚）**：控制台那两个主机名是 **Worker 自定义域名**
  （`GET /accounts/<acc>/workers/domains`），都指向 `vale-gate`；`vale-gate-wasm` 没有域名、`workers_dev: false`。
  它只服务 `/v1/*` 与 `/api/health`，所以切流不是换域名，而是**加两条更具体的 zone route**（更具体的 pattern 优先）：

      POST /zones/<zone>/workers/routes   {"pattern":"<console-host>/v1/*","script":"vale-gate-wasm"}
      POST /zones/<zone>/workers/routes   {"pattern":"<console-host-2>/v1/*","script":"vale-gate-wasm"}

  **回滚 = 删掉这两条**（自定义域名立刻接管，无需重新部署）：

      DELETE /zones/<zone>/workers/routes/<id>

  验证各一条：`curl -s https://<console-host>/v1/models | head -c 200` 应与发货逐字节相同；
  `curl -s -o /dev/null -w '%{http_code}' https://<console-host>/api/health` 仍由控制台那条服务（200）。
  **这一步要操作者点头**：它是改线上前门，不是这个仓库里的一个提交。

  **主机名在这里是占位符，而且不要把它们写回来**：`agent/tests/production_host.rs` 拒绝在文件里出现未申报的线上
  主机名（实测：这份文档写了两个，CI 的 `agent` 与 `pack-chain` 两个 job 一起变红）。真要写进去，得同时把这个文件
  加进 `ALLOWED` **并把 `MAX_ALLOWED` 加一**——而那张表只能缩，所以占位符才是对的答案。

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
