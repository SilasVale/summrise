# User-first Product Roadmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Summrise 按“免部署上手、权限清晰、任务可靠、身份与证据可迁移”的方向推进到个人版和小团队产品。

**Architecture:** 保留 Device 执行、Console 连接/控制、只读页面与桌面宿主分层。官方提供稳定公网入口；工作空间隔离与设备信任先闭合，再开放多人接入。签名身份、授权和任务收据先链下实现，公开证明为可选项目。

**Tech Stack:** 现有 Rust/Tokio/Axum、React/TypeScript、WASM 单 HTML、Electron/Windows runtime；Cloudflare 作为官方入口可选基础设施。未选定新链、币、云数据库或付费服务。

**Spec:** [产品方向与路线图](../specs/2026-11-23-user-first-product-direction.md)。本文件是里程碑和近期独立工作包，后续托管、工作会话、收据分别形成自己的接口/测试实施计划，避免把多个独立子项目混成一次重写。

## Global Constraints

- 普通用户默认无需域名、服务器、Cloudflare 账号或钱包；本地和自托管保持可用。
- 执行发生在用户选定的机器上；官方托管不暗示设备始终在线。
- 四个 A3 主入口与浅色层次保持，默认用户流程不暴露事务/传输/签名内部细节。
- 当前 Console 不能直接作为未隔离的多用户服务开放；所有服务端入口共享工作空间归属检查。
- 只读工作不以启动 Shell 为前置条件；目录限制必须由实际能力强制执行。
- 网络超时保持确定未发送/结果未知的区别；不自动重试未知结果的副作用。
- 撤销阻止后续访问；运行中的程序与已经发生的写入有独立处理语义。
- 设备密钥、令牌、文件、命令与输出正文不进入公链；签名不等价于执行正确性证明。
- 唯一维护分支为 main；当前发布保留未验收 Native 提案及现有数据与身份。
- 里程碑按实际验收推进，规划中的比例/时长/配额不是已测结果。

## Review Focus

1. 无域名、不懂终端的新用户能完成一次实际任务，而非停在配置页；归属 Task 3。
2. 第二个用户、过期配对码、猜测的设备/截图/审计地址不能跨空间读取或执行；归属 Task 2。
3. 丢响应后任务可能仍在执行，重复点击、重连和新 token 都不能造成误归属或静默重复；归属 Task 4。
4. 撤销、所有权迁移和密钥丢失后，旧发行者不能继续获得新的有效授权；归属 Task 2、5。
5. 收据被篡改、仅有 Console 观察或缺少设备答复时，验证器不能显示设备确认成功；归属 Task 5。

---

## Task 1：完成当前发布与可用性基线（M0）

**Files:** 复用当前 [CI](../../../.github/workflows/ci.yml)和[布局契约](../specs/layout-contract.json)作为验收入口；产品重点为 [App](../../../device/console/web/src/App.tsx)、[Connect](../../../device/console/web/src/ConnectSheet.tsx)、[调用解码](../../../device/console/web/src/terminal.ts)、[RunForm](../../../device/console/web/src/RunForm.tsx)、[Console](../../../device/console/src/http.rs)、[官网](../../../landing/public/index.html)和现有文档。

**Interfaces:** 消费当前 Device/MCP 与签名发布协议；产出精确 main SHA、匹配的构建/发布收据、线上版本与资产校验结果及真实平台状态。

- [ ] 将已完成的 Activity 与 failed-run/交付状态回归、首次调用文档修正纳入同一候选清单，审阅最终 diff。
- [ ] 在锁定依赖的验证副本执行完整 typecheck、unit、构建和既有浏览器范围；区分仪器修复、模拟场景与真实平台验收。
- [ ] 完成 Rust/Linux、Windows 构建与已有生命周期检查；未实测的平台能力保持明确状态。
- [ ] 验收精确提交后合入并非强制推送 main，等待该 SHA 的实际 CI。
- [ ] 按现有签名流程发布完整产物、部署官网与 Console；核对运行版本、嵌入页的版本替换后 SHA 和公网资源。
- [ ] 记录当前初次安装/连接的人工步骤数与失败入口，作为 M1 对照；不虚构新用户转化率。

## Task 2：先完成官方托管的隔离与信任设计（M1 前置）

**Files:** 读取 [auth](../../../device/console/src/auth.rs)、[HTTP](../../../device/console/src/http.rs)、[devices](../../../device/console/src/devices.rs)、[codes](../../../device/console/src/codes.rs)、[tokens](../../../device/console/src/tokens.rs)、[channels](../../../device/console/src/channels.rs)、[store](../../../device/console/src/store.rs)及 [Device trust](../../../device/device/src/console.rs)。新增独立设计 `docs/superpowers/specs/2026-11-23-hosted-workspace-boundary-design.md` 与其后独立实施计划。

**Interfaces:** 消费经过验证的登录身份与现有 Device 主动连接；设计产出稳定 principal、workspace、设备 owner、配对码发行者、token/grant 归属、流/截图/结果分区及签名信任契约。个人 workspace 初期只有一个成员。

- [ ] 完整列出从登录到 fleet、MCP、设备连接、截图、运行结果、审计流的入口及身份传递。
- [ ] 明确真实身份校验、空间选择、用户切换、配对/迁移/恢复与拒绝行为；只信任已验证身份，不信任外部自报邮箱。
- [ ] 比较请求级租户上下文与独立实例的最小端到端试验，验证签名钥匙隔离和 Device 公钥 pin；选择能通过隔离与运维验证的内部拓扑。
- [ ] 独立实施计划先列两个空间的反例：A 凭据访问 B 设备、B 审计/截图地址、旧配对码重放、跨空间令牌、旧发行者迁移后调用，均须拒绝且不泄露 B 内容。
- [ ] 在受控环境完成真实两个空间与设备的链路，再将官方入口开放为邀请内测；不以同进程两个列表代替隔离证明。

## Task 3：做通免部署的第一件事（M1）

**Files:** 重点为 [安装脚本](../../../device/installer/summrise.nsi)、[设备面板](../../../device/device/src/panel.rs)、[App](../../../device/console/web/src/App.tsx)、[Overview / Machines](../../../device/console/web/src/HomePage.tsx)、[Connect](../../../device/console/web/src/ConnectSheet.tsx)、[core registry](../../../device/core/src/registry.rs)及其功能测试。工作上下文拆分需另写独立设计后确定新增模块，不将其塞入 UI 文案。

**Interfaces:** 消费 Task 2 的账号/空间与配对；产出安装→登录→配对→连接客户端→只读示例→可见结果的完整路径。工作上下文不生成 OS shell。

- [ ] 画出六个首次使用状态：未安装、未登录、未绑定、机器离线、未连接 AI、首任务完成；每个只保留一个明确下一步。
- [ ] 把逻辑工作会话与终端资源分离，先设计/验证只读 credential 无 Shell 也能建立工作上下文并读允许文件的反例与成功用例。
- [ ] 集成稳定官方地址、一次性配对、客户端配置生成/复制；确认安装目录、系统账户执行权限和令牌用途的说明真实。
- [ ] 建立客户端兼容矩阵：先实际通过一种桌面和一种远端客户端，需要 OAuth 的客户端按其协议验收。
- [ ] 进行十次新用户首次使用演练；目标八次在十分钟内无工程师帮助完成真实只读任务。记录失败步骤，未达目标先修流程再加功能。

## Task 4：建立可恢复任务和明确失败结果（M2）

**Files:** [runs capability](../../../device/device/src/capabilities/runs.rs)、[bus](../../../device/bus/src/lib.rs)、[record](../../../device/record/src/lib.rs)、[Device channel](../../../device/device/src/console.rs)、[Console channel](../../../device/console/src/channels.rs)、[RunForm](../../../device/console/web/src/RunForm.tsx)、[Activity](../../../device/console/web/src/ActivityTimeline.tsx)。新增独立任务协议设计和实施计划。

**Interfaces:** 消费 workspace/设备/权限身份与现有 trace；产出稳定任务 ID、接收/运行/完成/失败/未知状态、受限输出游标及同一任务查询。幂等键须由执行端确认，不依赖前端按钮状态。

- [ ] 将已发现的 bus lag/游标边界、UTF-8 历史及服务读取所有权问题分别做确定性反例并最小修复，保持真实错误和 retention/reset 语义。
- [ ] 定义长任务状态查询与重复请求规则，验证请求已接受但响应丢失、同键重试和断网重连不会重复启动。
- [ ] 确定输出上限、截断、保留和查询状态，禁止把“没有读到”补成空结果；副作用未知时给出查询/检查记录入口。
- [ ] Activity 定位到准确机器、任务和已有会话；目标结束/离线时不能默默切换到另一会话。
- [ ] 在设备休眠、连接中断、代理超时、token 到期及退出账户下执行完整恢复演练，再公开可靠性承诺。

## Task 5：授权卡与可验证收据（M2，公开证明延后）

**Files:** 复用 [core grant](../../../device/core/src/grant.rs)、[Device admission](../../../device/device/src/remote.rs)、[protocol](../../../device/protocol/src/lib.rs)、[Activity](../../../device/console/web/src/ActivityTimeline.tsx)及当前签名/记录组件。先形成独立 receipt/grant 扩展设计；不直接将规划当成 DID/VC 合规实现。

**Interfaces:** 消费任务身份、设备身份、权限与执行结果；产出可读授权说明、可撤销后续访问、设备签发的离线可验收据及验证结果。Console 观察与设备确认分别标识。

- [ ] 定义授权卡可表达并由 Device 执行的约束；目录限制与终端 OS 权限分别表述，委托只可缩小。
- [ ] 测试到期、撤销、扩大委托、跨机器使用和运行中撤销，明确后续访问与已有程序/写入的区别。
- [ ] 确定带领域分隔和版本的收据格式、规范序列化、签发 key、轮换/恢复与隐私字段；不在日志持久化输出正文作为附带变更。
- [ ] 验证离线验签、篡改一字、错误 key、缺设备答复、仅 Console 观察、真实失败和截断输出；签名验证不显示为执行正确性证明。
- [ ] 先交付链下导出/验证器；有真实多方见证需求时再单独评估透明日志或链上批次根，网络/链故障不得成为执行入口依赖。

## Task 6：小团队、迁移与规模验证（M3 / M4）

**Files:** 基于 Task 2 的 workspace 归属层扩展成员/角色；读取 [mesh](../../../device/mesh/src/lib.rs)、[token 管理](../../../device/console/src/tokens.rs)、[连接](../../../device/console/src/channels.rs)和 [部署流程](../../../scripts/console-deploy.sh)。各项目另写独立设计，沿同一 main 集成。

**Interfaces:** 消费已验证的个人 workspace、授权与收据；产出可审计协作、受控所有权迁移及实际负载/成本记录。

- [ ] 先验证邀请、角色变化、成员移除与共享设备，保证个人空间和团队空间切换不继承错误权限。
- [ ] 设计用户授权的自托管/官方迁移与旧发行者撤销；不复制通用平台签名私钥到用户实例。
- [ ] 只在适用客户端与网络下接入已有 mesh，实测直连失败后中继回退、重连和原任务身份。
- [ ] 在受控负载下测长连接、截图/输出流量、记录保留和恢复成本，再确定资源额度与商业方案。
- [ ] 依据真实留存和协作需求决定下一批次；钱包登录、公开见证与算力市场各自作为独立可选项目评估。

## 执行约定

用户已经指定自主决策、多任务并行与只维护 main。当前阶段先完成 M0 并形成 M1 的隔离/首次使用设计；后续源写按独立任务、单文件所有者与资源租约推进。每个软件任务执行有意义的失败反例→最小实现→相同用例通过→相关回归→独立审阅→精确 main 发布；产品里程碑不能用截图、mock 或论文式设计代替用户路径验收。
