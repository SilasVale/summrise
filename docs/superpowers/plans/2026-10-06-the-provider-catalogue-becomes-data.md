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

- **A2 的真实状态（2026-10-07 复测）：不是"只差 authorized_keys 一行"，而是二进制还差两条活着的路由。**
  SSH 仍然被拒（`Permission denied (publickey)` ✓ 那一行确实还没加），但更要紧的是**本机的差分**：

  ```
  proxies/api-relay/relay $ node differential.mjs
    differential: 12/14 byte-identical, 2 route-not-wired, 0 unexplained
  ```

  两条"没接线"的是 `/api/github` 与 `/api/gform`——**而它们在 VPS 上是活的**（直接量）：

  | 探针 | 活的 vrelay | Rust 二进制 |
  |---|---|---|
  | `GET /api/github?path=%2Fx` | **400** `{"error":"unsupported GitHub route"}`（路由存在，只是路径不对） | **route not wired** ✗ |
  | `GET /api/gform?path=%2Fx` | **400** `{"error":"unsupported Google route"}` | **route not wired** ✗ |

  **所以照现在这样部署会打断两条正在服务的路由** ✓。仓库自己的测试也写着这件事
  （`relay/tests/two_relays_agree.rs:194`："github traffic to the git handler, via `/api/github/o/r`. Behaviour
  is the stronger check"）✓。

  **要做的事是明确的、而且有现成的判据**：把 `api/github.ts`（231 行）与 `api/gform.ts`（350 行）搬进
  `relay/src/lib.rs`（现有 2,862 行），**判据就是 `differential.mjs` 从 12/14 变成 14/14** ✓——这套差分
  （oracle 驱动发货 TS、二进制跑 Rust、逐字节比）已经在仓库里，所以这是一次"搬到差分全绿为止"的活，不是探索。
  relay 自己的测试现在是绿的：`cargo test` 28 + 6 + 15 = **49 passed** ✓。

- **`gform` 的接线清单（2026-10-07 逐项核对，判据仍是 `differential.mjs` 到 14/14）。**
  `/api/github` 已经接上（差分 12/14 → **13/14** ✓），`gform` 是最后一条。**已经搬好、已被证明的**：
  `GFORM_UPSTREAMS`（8 个上游 ✓）、`rewrite_body` ✓、`rewritable` ✓、`set_cookie_values` ✓、
  `form_decode`/`form_encode` ✓，以及**通用的** `upstream_url(base, path)` ✓（它收 base 作参数，所以直接可用）。
  **还缺的四块**（都是小件，但每一件都必须逐字对上，否则差分不会到 14/14）：

  | 缺什么 | 为什么不能复用现有的 | 量 |
  |---|---|---|
  | `GFORM_ALLOWED_REDIRECT_HOSTS`（8 个 Google 主机）+ `gform_redirect_target` | `redirect_target` 带的是 **GitHub 的 7 个主机** ✗ | ~20 行 |
  | `gform_parse_route` | crate 的 `parse_route` 用 **github 的 `UPSTREAMS`** ✗（gform 是 gle/docs/www/gstatic/ssl-gstatic/fontscss/fonts/usercontent 八个） | ~15 行 |
  | gform 自己的 `copy_request_headers` / `copy_response_headers` | 两份白名单与 github 的**不同** ✗（请求侧多 `content-type`、少 `accept-encoding`；响应侧是它自己的 5 项） | ~10 行 |
  | 外壳 `gform()` | 方法闸门是 GET/HEAD/**POST** ✗（github 只有 GET/HEAD）；还有 reCAPTCHA 的 cookie 规则（`www.google.com` + `/recaptcha/` 前缀：请求侧透传调用者的 `cookie`、响应侧**追加全部** `set-cookie`）、304 空体、`MAX_REWRITE_BYTES = 10 MiB` 以上跳过改写直接流、5xx 通用文案 | ~120 行 |

  **合计约 165 行**，每一块都有差分里的对应案例在盯着 ✓——所以这是"搬到 14/14 为止"，不是探索 ✓。

- **B 的删除清单：每个渠道的"没人用"证据（2026-10-07 实测），以及删掉它会发生什么。**
  目标是"每删一个渠道先量它没人用（KV 的 ukeys 记录 + 访问日志 + 操作者确认）"——**访问日志这一项拿不到**：
  网关的 `observability` 是关的，所以能拿到的只有 KV 与操作者的确认，下面就是这两样：

  | 渠道 | 有人的 BYOK key 吗（生产 KV 的 `ukeys:` 记录） | 部署的 env secret | 健康卡 | 删掉的后果 |
  |---|---|---|---|---|
  | `og/` | **有**（`ukeys:admin`） | ✓ 两个 worker 都有 | 6 | **在用（操作者 2026-10-06 明确）**——不动 |
  | `ds/` | **有**（admin、test） | ✓ | 0 | 有人的 key 会失效 |
  | `or/` | **有**（admin、test） | ✓ | 4 | 同上 |
  | `nv/` | **有**（admin） | ✓ | 1 | 同上 |
  | `cm/` | **有**（admin） | ✓ | 3 | 同上（而且它是 `/api/health` 的 `recommended`） |
  | `qw/` | **没有** | ✓ | 3 | **只影响"用部署的 key"这条路径**——而操作者说没人用 |
  | `gmi/` | **没有** | ✓（vale-gate）/ ✗（wasm） | 2 | 同上 |
  | `amd/` | **没有** | ✓（vale-gate）/ ✗（wasm） | 0 | 同上 |
  | `r4/` | **没有** | ✓（vale-gate）/ ✗（wasm） | 1 | 同上 |

  **读法**：生产 KV 里只有两条 `ukeys` 记录（`admin`、`test`），它们持有 5 个渠道的 key
  （`CMD_API_KEY`、`DEEPSEEK_API_KEY`、`NVAPI_KEY`、`OPENCODE_GO_API_KEY`、`OPENROUTER_API_KEY`）——
  **`qw/`、`gmi/`、`amd/`、`r4/` 在任何记录里都没有 key**，所以它们今天能服务只可能靠部署的 env secret，
  而操作者说只有 `og/` 在用。**这四个是唯一"删了不会让谁的 key 失效"的候选**，可以一轮一个（渠道 → 健康卡 →
  BYOK 键位 → 测试与语料），四个加起来 6 张健康卡（3+2+0+1）。

  **另外五个（og/ds/or/nv/cm）需要操作者一句话**：它们的 key 在 KV 里，删掉就等于让那个人的 key 失效——
  如果那些 key 是当初试过就没再用的，说一声就一起删；如果还在用，就留着。

- **② 的目标形状：逐字段核对（2026-10-07）。** 目标说的是"照 DSH 的 provider 记录"——所以这张表**逐字段**回答，
  每一行都给出它在哪里被实现、以及哪条测量证明它真的在起作用：

  | DSH 的字段 | 我们的拼写 | 实现处 | 证明它在起作用的测量 |
  |---|---|---|---|
  | `apiKeyEnv` | `apiKeyEnv` ✓ | `store/providers.ts` 的类型 + `store.rs` 的 `provider_key_env_names`（**逐记录动态读**，因为绑定名是任意的） | 扫描案例 "a NAMED binding's value rides Bearer"（3/3）：记录里写 `apiKeyEnv: ACME_TEST_KEY`，上游收到的就是那个绑定的值 |
  | `api` | `api` ✓ | `routing.rs` 的 `SUPPORTED_PROVIDER_APIS`（`openai-completions` → `/chat/completions`） | 案例 "a custom provider with an UNSUPPORTED api（the error route）"：不支持的方言走错误路由，**不是**静默回退 |
  | `baseURL` | `baseURL` ✓ | `routing.rs` 的 `provider_route`（**前缀**语义，方言的路径追加在后面——"OpenAI SDK 对 baseURL 做的事"） | 案例 "a custom provider whose baseURL carries a PATH"：`https://acme.test/openai/v1` + `/chat/completions` 拼出来的 URL 两边逐字节相同 |
  | `models[{id,name,contextWindow,maxTokens}]` | 同名字段 ✓（+ `vision`、`reasoningEffort`） | `store.rs` 的 `advertised_provider_models` + `/v1/models` | `GET /v1/models` 无 token 2,268 字节，**三种环境逐字节相同**（发货 TS、Node harness、真 workerd） |
  | 默认模型选择 | `resolveAutoModel` + `models:overrides` + `settings:` | `src/model-route.ts`（A3 后从 `plugins/` 搬到 `src/`） | `plugins.test.mjs` 的 "me/route: … GET shows stored + effective"（44 条健康/路由断言里的两条） |

  **所以②的"形状"部分是完成的**：记录自描述（前缀、标签、方言、上游根、key 的来源、模型清单与显示属性），
  Rust 侧读它、路由它、广告它，控制台写它。**剩下的是操作者那一半**：把不用的渠道删掉（B 的删除清单）——
  那需要先量"没人用"，而访问日志在网关侧是关掉的（`observability: false`），所以判据只能是
  KV 的 `ukeys` 记录 + 操作者的确认。

- **切流前最后一张表：部署层还缺什么，以及每一处缺口的实测答复（2026-10-07，在 workerd 上量的）。**

  `vale-gate-wasm` 比 `vale-gate` 少四个 secret。**每个缺口的答复是量出来的，不是推断的**（本地 KV 里种一个
  无渠道 key 的用户，再请求那个渠道）：

  | 缺的 secret | 渠道 | 候选 worker 实际答什么 | `/api/health` 看得出来吗 |
  |---|---|---|---|
  | `CMD_API_KEY` | `cm/` | **502** `config_error: CMD_API_KEY not configured — add your Command Code key in the console` | **看不出来**（cm 的 3 张卡都 `ok:true`） |
  | `GMI_API_KEY` | `gmi/` | **502** `config_error: GMI_API_KEY not configured — add your GMI Cloud key in the console` | **看不出来**（gmi 的 2 张卡都 `ok:true`） |
  | `R4_API_KEY` | `r4/` | **502** `config_error: R4_API_KEY not configured — add your own r4.codes key in the console` | **看不出来**（r4 的 1 张卡 `ok:true`） |
  | `AMD_API_KEY` | `amd/` | **502** `config_error: AMD_API_KEY not configured — add your AMD Radeon Cloud (rc-…) key in the console` | **看不出来**（amd 在健康里**没有卡**） |
  | `DO_AUTH` | 熔断读 | 读被拒 → `false`，于是 og 卡在**熔断已开**时仍报 `ok:true`（`reliability.ts` 自己的注释） | **看不出来**——这正是它说的那句 |

  **判据的主语**：`/api/health` 报的是"这些模型通不通"，不是"这些 key 在不在"——所以**带着四个缺失的 secret
  切流，健康面会 20/20 全绿，而其中三个渠道对每个没有自带 key 的用户答 502**。这不是健康卡的缺陷（改它就会
  偏离发货行为），是切流清单上必须先做的一步：`wrangler secret put <NAME> --name vale-gate-wasm`。

- **切流前最强的一条证据：发货前台（活的）与候选 worker（workerd 上）逐字节相同。** 公开的那个 console host
  可以直接量（另一个在 Cloudflare Access 后面，`/v1/*` 会 302/403）——**两个主机名都不写在这里**：
  `agent/tests/production_host.rs` 是一道**棘轮**，它只允许主机名出现在**一个已声明**的文件里（
  `gateway/wrangler.jsonc`），本轮我在本文件里写了两次、又在 `gateway/wasm/wrangler.jsonc` 里写了一次，两次都被它
  拒（第二次是 CI 拒的）。主机名一律写成 `<console-host>`：

  | 探针 | 活的发货 TS | workerd 上的候选 | |
  |---|---|---|---|
  | `GET /v1/models`（无 token） | 200，**2,268 B** | 200，2,268 B | **逐字节相同** |
  | `POST /v1/messages`（错 token） | 401，**97 B** | 401，97 B | **逐字节相同** |
  | `OPTIONS /v1/messages`（预检） | 200，`access-control-allow-origin: https://<console-host>`，`Vary: Origin` | 同 | **相同** |
  | `GET /api/health` | 200，**1,228 B** | 200，1,228 B | **逐字节相同** |

  **这一组和分歧扫描是两回事**：扫描比的是"同一台 Node harness 里的两份实现"，这四条比的是**真实部署与候选**，
  中间没有 harness（`cmp` 逐字节，不是长度相等）。四条都是不需要 token 的路径——带 token 的路径没有操作者的
  token 就不能这样量，那部分仍由扫描与语料负责。

- **A2 切流②**（relay 的 axum 二进制上 VPS）：本机跑通、六条路由 curl 过；只卡在盒子 `132.226.90.175` 的
  `authorized_keys` 一行。
- **A3 切流后删除** `gateway/src` 的 `/v1` 半边——**先证明等价，再删**。等价已经证明（分歧扫描 64/64、0 处已知差异），
  删除清单是 2026-10-06 按**导入图实测**的，不是估的：

  | 删（只被 `/v1` 用到） | 行数 | 谁还引用它 |
  |---|---|---|
  | `gateway/src/plugins/translate.ts` | 1,696 | 只有 `index.ts`（前门） |
  | `gateway/src/anthropic-translate.ts` | 798 | `translate.ts`、`translate-vision.ts` |
  | `gateway/src/body-scan.ts` | 379 | `translate.ts` |
  | `gateway/src/plugins/translate-vision.ts` | 313 | `translate.ts` |
  | **合计** | **3,186** | |

  **留下的（看着像 `/v1`、其实控制台在用）**：
  - `gateway/src/plugins/model-route.ts`（111 行）——它唯一的**导入者**是 `translate.ts`，但它的**导出**被控制台读：
    `plugins/auth.ts` 通过插件上下文取 `resolveAutoModel` 给 `GET /api/me/route` 用（`optionalApi(ctx,"translate")`，
    取不到就是 `null`）。删之前要把它接到直接 import，否则那条控制台路由会静默降级。
  - `upstream.ts`（489）、`reliability.ts`（456）、`channels.ts`（655）、`store/providers.ts`（542）——`tooling.ts`
    的健康/路由信息、`BreakerDO` 的导出、健康卡与模型注册表都要它们。

  **删完要做的事**：`index.ts` 去掉 translate 插件、`translateHandleGateway` 与 `/v1` 分派（`/v1/*` 由 zone route
  交给 wasm worker）；`index.ts` 的那三个 re-export 改指 `plugins/model-route.ts` 与 `reliability.ts`；测试里
  **只**为 `/v1` 存在的那几个随之删除（实测 18 个测试文件碰到这半边，同时覆盖共享模块的留下）；`route-oracle.mjs`
  随之退役（它驱动的就是这份 TS），`verify.mjs` 的比较端从"发货 TS"变成"已录语料"。
- **A4 JS 账（2026-10-06 接手时 → 2026-10-06 本轮，`git ls-files` + 逐文件行数）**：

  | | 接手时 | 现在 | Δ |
  |---|---|---|---|
  | Rust | 114,808 行 / 217 文件 | **116,377 行 / 218 文件** | **+1,569 / +1** |
  | JS 家族（TS+TSX+JS+MJS+CJS） | ~153,800 行 / 571 文件 | **155,180 行 / 571 文件** | **+1,380 / 0** |

  **JS 这一会话是涨的，涨在哪必须写在表上**（这不是"没有新增 JS"，是"新增的每一行都落在具名例外里"）：

  | 涨在哪 | Δ | 属于哪个例外 |
  |---|---|---|
  | `gateway/wasm/`（`verify.mjs` + `route-oracle.mjs`） | **+1,301** | 驱动 JS+wasm 产物的 harness（切流后 oracle 退役） |
  | `gateway/ui/`（提供商表单 + i18n + CSS） | **+79** | 浏览器 UI |

  其余目录这一会话**没动**：`gateway/src` 14,057（发货 TS 半边，A3 切流后删）、`gateway/test` 20,251、
  `agent/resources` 42,505、`agent/scripts` 8,910、`agent/summrise-agent-npm` 13,741、Electron 4,207、
  `proxies/api-relay` 3,916。

  **判据的主语**：`gateway/wasm` 里那 5,074 行 JS 家族**全部**是两个 harness（+ `loader.mjs`），没有一行产品逻辑；
  Rust 侧同期 +1,569 行里，`vision.rs`（一个完整特性）与四份新语料/回放是主体。**切流删掉 `gateway/src` 的 /v1 半边
  之后，`route-oracle.mjs` 与它驱动的差分一起退役**，这条数会掉。

  **A4 的账，2026-10-07 复测（A3 删完之后）**——同一套规则：`.rs` 不计 `target/`，JS 家族是
  `.mjs/.js/.cjs/.ts/.tsx` 不计 `node_modules/`，**镜像单列**（它是生成出来的副本，不是源码）：

  | | 接手时（10-06） | 现在（10-07） | Δ |
  |---|---|---|---|
  | Rust | 114,808 / 217 | **116,642 / 218** | **+1,834 / +1** |
  | JS 家族（含镜像） | ~155,180 / 571 | **142,885 / 530** | **−12,295 / −41** |
  | JS 家族（不含镜像） | ~139,300 | **126,998 / 523** | **−12,302** |

  **这 −12,295 行就是 A3**：3,186 行实现（`plugins/translate.ts` 等四个文件）+ 3,454 行 oracle（`route-oracle.mjs`
  与两个 `oracle*.mjs`）+ 5,432 行测试（13 个主题就是那半边的文件），再减去改指时加回来的注释与
  `frontDoor()`（`gateway/src` 只掉了 3,144 而不是 3,186，差的就是这些）。

  | 目录 | 行数 | 为什么是 JS/TS（或为什么它不再是） |
  |---|---|---|
  | `gateway/src` | 10,913 | Cloudflare Workers 跑 V8；**但 `/v1` 半边已经不在里面了**——只剩前门 4 行分派 + 控制台 `/api/*`（A6 的候选） |
  | `gateway/ui` | 9,775 | 浏览器 UI（TS/TSX，零 `.js`） |
  | `gateway/wasm` | **2,583** | **只剩 harness**（`verify.mjs` 等），一行产品逻辑都没有；oracle 随被删的 TS 一起退役了 |
  | `agent/scripts` | 8,909 | 判据 + Playwright 驱动：驱动必须留 JS（浏览器），**判据是 A5 的候选** |
  | `agent/resources/panel-react` | 42,404 | 浏览器 UI |
  | `agent/summrise-agent-npm` | 13,741 | npm 是 CLI 的交付方式 |
  | `agent/summrise-desktop-electron` | 4,207 | Electron 壳 |
  | `index` / `proxies` | 6,085 / 7,819 | CDN worker 与卫星 worker（Workers 运行时）——**其中 Rust 的部分已经在 Rust 里** |
  | `gateway/public/code/files` | 15,887 | **生成物**：Source Viewer 的镜像（`sync-code-viewer.sh` 生成，随源文件一起变） |

  **判据的主语没变**：`gateway/wasm` 的 2,583 行 JS 全是 harness（驱动 JS+wasm 产物 ✓，是目标里写明的例外）；
  `gateway/src` 的 10,913 行里已经**没有 `/v1` 逻辑**——那部分现在在 Rust 里跑在生产上 ✓。

  **覆盖表（2026-10-06，`verify.mjs` 的 83 个案例按 渠道 × arm 数出来）**——`·` 是**没有被比较过**的格子：

  | 渠道 | `/v1/messages` | `/v1/chat/completions` | `/v1/responses` | `count_tokens` |
  |---|---|---|---|---|
| `og/` | 34 | 2 | 2 | 3 |
| `ds/` | 2 | 2 | **·** | 1 |
| `qw/` | 1 | 1 | **·** | 1 |
| `or/` | 1 | 1 | **·** | **·** |
| `nv/` | 2 | 1 | **·** | 1 |
| `gmi/` | 2 | 1 | **·** | 1 |
| `cm/` | 1 | 1 | **·** | 1 |
| `amd/` | 1 | 1 | **·** | 1 |
| `r4/` | 1 | 1 | **·** | 1 |
| `acme/` | 6 | 1 | **·** | 1 |

  **剩下的 10 个空格里，9 个是 `/v1/responses`**——那条 arm 只服务 og 的 muse 模型（源码："Serves
  og/muse-spark-1.2/1.3-contributor ONLY"），所以别的渠道没有案例是**正确**的覆盖，不是缺口；`or/` 的
  `count_tokens` 是唯一真的空格。**这张表是"89/89"的正确读法**：它说的是"这 83 个已命名的案例里没有分歧"，
  不是"移植完成了"——`gmi/` 的翻译分支就是在 64/64 看起来完整的时候被这张表找出来的。
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
