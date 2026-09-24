# 排谷机器人 · 程序设计路线（DESIGN）

> 与 [POLICY.md](./POLICY.md) 配套。本文件定义**模块、接口、数据结构、路由、部署**。
> 所有子 agent 必须遵守本文的**文件所有权**与**接口契约**。

## 1. 总体架构

```
                         (真实群 123456789)
 NapCatQQ ──reverse WS──▶ Gateway  0.0.0.0:9801
                              │  白名单/drop · 心跳 · 动作回包(只读)
                              ▼
                    Intake Queue (有界 mpsc)
                              ▼
   Pipeline:  幂等 → 规则快速路径 → LLM 清理/抽取 → 校验 → 权限 → 分配 → 回复(默认关闭)
                              │                         ▲
                              │                         │ Hot Config (config/app.json, revision)
                              ▼
        Engine/Replay(既有) → Snapshot → HTTP/展示（发布器已随旧栈删除，R2/Pages 待接线）
                              │
        axum HTTP API :21081 ─┼─ /api/config /api/board /api/display /api/messages
                              ├─ /api/sim/*  (模拟器，仅本地)
                              ├─ /api/members(/refresh)
                              └─ /web/* 静态页 (display/admin/sim/replay)
                              ▼
        Cloudflare: R2(快照/回放) + Pages(静态展示页)
```

**运行模式**：本地一个进程同时跑 Gateway + HTTP API + Pipeline；`cargo run -- run`（默认）。
保留既有 `simulate`（离线重放验证；旧 verifier 暂留以支撑语料回归，见 T-24）与 `serve`（已并入新栈，等价 `run`）子命令；展示统一走 HTTP API。

## 2. 目录与文件所有权

| 路径 | 内容 | 归属 |
|---|---|---|
| `src/gateway/` | OneBot 协议、路由(白名单/drop)、WS server、动作回包 | **A1** |
| `src/llm/` | OpenAI 兼容客户端、排谷流水线 | **A2** |
| `src/settings.rs` | `AppConfig` + `ConfigStore` 热载存储(revision) | **A2** |
| `src/bus.rs` | 冻结接口：`IncomingEvent` / `EventSink` / `PipelineOutcome` | **A0** |
| `src/api/` | axum 路由：config/board/display/messages/sim/members/gateway + 静态页 | **A3** |
| `web/` | 静态页 display/admin/sim/replay（vanilla，可部署；`viewer/` 已合并至此） | **A4** |
| `tests/e2e/` | Node `.mjs` Playwright e2e | **A5** |
| `src/main.rs`、各 `mod.rs`、`src/error.rs` | 装配与接线 | **A0(主)** |
| `src/engine/**`、`src/replay/**`、`src/parser/**`、`src/simulation/**` | 既有引擎（改动需 A0 同意） | 主 |
| ~~`src/config.rs`、`src/inbound/**`、`src/ws/**`、`src/services/**`、`src/repo/**`、`src/publisher/**`、`src/api/routes.rs`~~ | **旧栈：已在 C-4 删除**（`dev`→`master` 合并）；现役为 `src/gateway/**` 与 `src/api/*_routes.rs` | — |

> 子 agent **不得**改他人归属文件；需要跨模块改动时在 `docs/TASKS.md` 记录并等 A0 处理。

## 3. 配置（`config/app.json`，热载）

```jsonc
{
  "revision": 1,
  "active_round_id": "月行水上",       // U3：指向 data/rounds/月行水上.json（启动/热载据此覆盖 round）
  "gateway": {
    "bind": "0.0.0.0:9801",
    "whitelist_groups": ["123456789"],
    "heartbeat_secs": 15,
    "reply_enabled": false,          // 默认 false：绝不发送；send_* 仅当 true 且在白名单时放行（已强制）
    "allowed_actions": ["get_group_member_list","get_group_info","get_group_list","get_login_info"]
  },
  "llm": {
    "enabled": true,
    "base_url": "https://api.deepseek.com",
    "model": "deepseek-flash",
    "api_key_env": "DEEPSEEK_API_KEY",
    "timeout_secs": 60,
    "max_tokens": 2048,
    "temperature": 0.1,
    "fallback_to_rules": true,
    "prompt_template": "<中文提示词，见 §6>"
  },
  "round": {
    "round_id": "月行水上",
    "title": "月行水上",
    "group_id": "123456789",
    "priority_users": ["user_a","user_b","user_c","user_d"],
    "priority_window": { "start_ms": 1788782400000, "end_ms": 1788789600000 },
    "items": [ /* 见 §4 */ ]
  },
  "display": { "refresh_ms": 5000, "data_source": "local", "remote_base_url": "" },
  "members": {
    "group_id": "123456789",
    "cache_path": "data/members.json",
    "daily_pull_at": "19:00",
    "cn_overrides": []               // U4：成员具体名（CN），[{user_id, cn, aliases[]}]
  }
}
```

- **轮次库（U3，2026-09-24）**：`round`（运行时激活轮次）由 **`active_round_id` → `data/rounds/<round_id>.json`** 驱动；启动/热载解析见 `src/settings/rounds.rs::resolve_active`（文件存在则覆盖 `round`；否则把当前 `round` 落盘并回填 `active_round_id`）。目录可由 `PAIGU_ROUNDS_DIR` 覆盖（默认 `data/rounds`）。`config/app.json` 不再内联完整商品目录，仅保留 `active_round_id`。

- 存储：`ConfigStore`（`tokio::sync::RwLock<AppConfig>` + `revision`）；`PUT` 时校验 `revision`，成功 `revision+1`；`notify` 监听文件变更自动热载。
- 环境变量覆盖：`PAIGU_CONFIG_PATH`（默认 `config/app.json`）、`PAIGU_HTTP_PORT`（默认 `21081`）。
- 成员名单读取顺序：**刷新缓存 `data/members.json` → `data/members.seed.json`（gitignored，真实名单，支持 `[...]` 或 `{"members":[...]}`）→ `data/members.example.json`（入库占位）→ 空数组**；`/api/members` 的 `source` 为 `cache|seed|example|empty`，每项附 `cn`/`resolved`（U4）。路径可用 `PAIGU_MEMBERS_SEED_PATH` / `PAIGU_MEMBERS_EXAMPLE_PATH` 覆盖。

## 4. 商品目录（`round.items`，种子来自 `simulation-corpus/real-chat/月行水上`）

> ✅ **已更新（2026-09-24，见 [SPEC-UPDATE-2026-09-24.md](./SPEC-UPDATE-2026-09-24.md) §U6；源码 `src/settings/mod.rs`）**：**商品级只有原价**；**仅变体**有 `adjust_cents`；`class` **自动推导**；**种类**为 `拼团/单领/整盒/特典`。

**现行字段表**（`ItemConfig` / `VariantConfig`）：

| 层级 | 字段 | 类型 | 含义 |
|---|---|---|---|
| 商品 | `item_id` | String | 稳定 id（唯一） |
| 商品 | `name` | String | 展示名 |
| 商品 | `kind` | String | **种类**：`拼团`/`单领`/`整盒`/`特典`（兼容 `group/single/box/gift` 与旧 `split/single/gift`；UI 用「种类」） |
| 商品 | `class` | Option<String> | `A`/`B`；**缺省自动推导**（有变体⇒A，无变体⇒B），显式合法值可覆盖 |
| 商品 | `aliases` | String[] | 商品名别名（本地词组切分 + LLM 建议 + 跨商品冲突校验） |
| 商品 | `unit_price_cents` | i64 | **原价**（分）；商品级**无**调价 |
| 商品 | `max_quantity` | Option<u32> | **单领上限**（仅单领；不出现在拼团商品） |
| 变体 | `variant_id` | String | **只读、自动生成**（不允许手填） |
| 变体 | `name` | String | 变体名（**可改**） |
| 变体 | `unit_price_cents` | i64 | **原价 A**（分） |
| 变体 | `adjust_cents` | i64 | **调价 B**（分）；最终价 **C = A + B**（见下方实现注记） |
| 变体 | `capacity` | Option<u32> | 容量（可空） |
| 变体 | `aliases` | String[] | 变体别名 |

> **移除口径**：`box_size` 与 `variants[].pieces` 已从**商品模型口径**移除（拼团逐个设置变体；单领/整盒不考虑内容）。⚠️ 实现注记：Rust `ItemConfig.box_size` / `VariantConfig.pieces` 字段目前**仍保留为兼容反序列化**（编辑器不再读写），且 `config.example.json` 仍含旧值 —— 清理项见 [TODOS.md](./TODOS.md) W-G2-04。
> ⚠️ **`adjust_cents` 实现注记**：`/round` 编辑器（`web/round.js`）已按 `原价 A ± 调价 B = 最终价 C` 三格联动读写 `variants[].adjust_cents`；但 Rust `VariantConfig` **尚未声明该字段**（当前反序列化时被忽略、不落 `data/rounds/*.json`）。落库/结算接线见 [TODOS.md](./TODOS.md) W-G2-01。

```jsonc
{ "item_id":"pass_sp", "name":"通行认证SP-月行水上", "kind":"拼团",
  "class": null,                       // 自动推导：有变体 ⇒ A
  "aliases":["通行证","通行认证SP","通行认证"],
  "unit_price_cents": 0,               // 商品级原价（有变体时通常为 0，价格挂变体）
  "variants":[
    {"variant_id":"v_jcl","name":"结城理","unit_price_cents":5000,"adjust_cents":0,"capacity":null,"aliases":[]},
    {"variant_id":"v_yy","name":"岳羽由加莉","unit_price_cents":5000,"adjust_cents":0,"capacity":null,"aliases":[]},
    {"variant_id":"v_ajs","name":"埃癸斯","unit_price_cents":5000,"adjust_cents":0,"capacity":null,"aliases":[]},
    {"variant_id":"v_hlw","name":"虎狼丸","unit_price_cents":2500,"adjust_cents":0,"capacity":null,"aliases":[]}
  ] }
```
另含：`hr_resume`(人事部简历SP-月行水上)、`fashion`(风尚速递SP-月行水上)、`gift_card`(特典卡组-校园凭证，变体含 `整套`，别名 `一套`)。

**编辑器（`/round`，2026-09-24）**：4 种类选择；变体 A±B=C 三格联动；别名本地词切 + LLM 异步建议（状态：等待回复/已填入新方案/是最佳）+ 锁定（人工编辑后自动上锁）；跨商品冲突校验（失焦/切换面板时检查，后端 `POST /api/rounds/:id/check` 兜底）；末尾虚线卡 + 缝隙「+」插入；单领 1 tab 多行；整盒独立种类；消息日志编辑器为排谷界面**折叠面板**。

## 5. HTTP API（`127.0.0.1:21081`）

> ✅ **已更新（2026-09-24）**：本表为摘要，**完整接口/字段以 [INTERFACES.md](./INTERFACES.md) §8（尤其 §8.7）为准**。下表原有行 + 2026-09-24 新增行如下。

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/health` | 健康检查 |
| GET | `/api/config` | `{config, revision}` |
| PUT | `/api/config` | body `{config, revision}` → 成功 `{config, revision}`；陈旧 → 409 |
| POST | `/api/config/reload` | 从磁盘强制重载 |
| GET | `/api/board` | 当前排位快照 + 状态 |
| GET | `/api/display?since=<version>` | 增量：`{version, board, messages, who_whats, status, changed}` |
| GET | `/api/messages?since=<seq>` | 增量消息流 |
| POST | `/api/sim/message` | `{user_id, nickname, text, offset_ms, group_id?, is_admin?}` → `{outcome, board, version}` |
| POST | `/api/sim/identity` | `{user_id, nickname, is_admin?, priority?}` |
| POST | `/api/sim/reset` | 清空模拟会话 |
| GET | `/api/members` | 成员列表（刷新缓存 → `data/members.seed.json` → `data/members.example.json` → 空；每项附 `cn`/`resolved`） |
| POST | `/api/members/refresh` | 经 Gateway 拉取 `get_group_member_list` |
| GET | `/api/gateway/status` | WS 连接状态 |
| GET | `/api/replay`、`/api/replay/*` | 逐步重放（Wave 1–4 已实现；路由/返回值见 [INTERFACES.md](./INTERFACES.md) §8） |
| GET | `/` `/admin` `/sim` `/replay` `/settlement` | 静态页（display / admin / sim / replay / settlement） |
| GET | `/round.html`、`/settings.html` | 静态页（轮次与商品 / 其余配置）；**无 `/round`、`/settings` 短路由**，由静态文件服务提供 |
| GET | `/api/workflow` | 只读工作流快照（含 `phases`）；见 [INTERFACES.md](./INTERFACES.md) §8.6/§8.7 |
| GET/POST | `/api/rounds` | 轮次列表 / 新建（`{round_id,title?,copy_from?}`） |
| POST | `/api/rounds/:id/activate` | 切换激活（`{mode?:continue\|fresh\|replay}`，默认 `continue`） |
| POST | `/api/rounds/:id/check` | 轮次结构校验（`{ok,issues[]}`） |
| DELETE | `/api/rounds/:id` | 删除轮次（激活中拒绝） |
| POST | `/api/items/suggest-aliases` | **前端已接入、后端已实现**（UI 404/501 兜底）；见 §8.7 与 [TODOS.md](./TODOS.md) W-G2-02 |
| GET | `/web/*` | 静态文件；未命中时 `fallback_service` 回退到目录 `web/` |

CORS：本地开发允许 `http://127.0.0.1:*`；远程展示页读 R2，不经此 API。

## 6. LLM 提示词（初始草稿，admin 可改）

系统提示（要点）：
- 你是排谷消息解析器；只抽取，不计算价格/排序/回复。
- 输入：群消息文本 + 当前商品目录 + 预存用户 + 时段政策。
- 输出：严格 JSON（见 POLICY §3 契约）。
- 非排谷消息 → `intent=unknown`。
- 歧义 → 填 `ambiguous_parts`。
- 中文口语别名映射（通行证→通行认证SP、特典→特典卡组-校园凭证、一套→整套 等）。

## 7. 实时性与一致性

- 入口 push；读循环仅解析+入队（有界 `mpsc`，满则记背压日志）。
- 入队时分配单调 `sequence`（全局 `AtomicI64`），保证并发消息重放确定性。
- LLM 在独立 worker（并发上限可配）；超时回退规则。
- 展示 5s 增量轮询；`version` 单调；前端 keyed diff。
- 发布：R2/Pages 远程发布**未接线**（`src/publisher/**` 已随旧栈在 C-4 删除；`domain::snapshot::Public*` 视图模型保留）。

## 8. 成员名单（占位示例）

内置/示例成员使用占位名（`成员01`…`成员05`），入库文件 `data/members.example.json`；
真实名单放在 gitignored 的 `data/members.seed.json`（支持 `[...]` 或 `{"members":[...]}`）。
`/api/members` 读取顺序 **刷新缓存 → seed → example → 空**，`source` 为 `cache|seed|example|empty`。
**成员具体名（CN，U4）**：`members.cn_overrides[{user_id, cn, aliases[]}]`；展示人名回退 **CN → 归一化昵称 → user_id**（`settings::resolve_cn`），每项附 `cn`/`resolved`；用于匹配与结算表/账单人名。

```
成员01, 成员02, 成员03, 成员04, 成员05
```
（占位名单可按需扩展；真实昵称不得写入被 git 跟踪的文件。）

## 9. 部署（将来）

- 本地：`cargo run`（Gateway + API + Pipeline），数据落 `data/`。
- 远程：`web/display` 静态页部署 Cloudflare Pages；快照/回放发布 R2；页面按 `display.data_source=remote` 读 `remote_base_url` 下 `rounds/{round_id}/current`（**无 `.json` 后缀**，与后端 `GET /rounds/{id}/current` 契约一致；实现见 `web/common.js` 的 `resolveRemoteCandidates`）。
- 展示页需支持两种数据源适配器（local/remote），同一套渲染。

## 10. 测试

- Rust：路由(白名单/drop)、配置 revision/热载、流水线(规则/LLM mock)、权限时段。
- Node `.mjs` Playwright：驱动 `/sim` 页面 → 选身份/设偏移/发消息 → 断言排位与状态；覆盖时段拒绝与预存优先。
- 回归：既有 `cargo test`（139）与 `simulation-corpus` 脚本必须保持通过。
