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
  "members": { "group_id": "123456789", "cache_path": "data/members.json", "daily_pull_at": "19:00" }
}
```

- 存储：`ConfigStore`（`tokio::sync::RwLock<AppConfig>` + `revision`）；`PUT` 时校验 `revision`，成功 `revision+1`；`notify` 监听文件变更自动热载。
- 环境变量覆盖：`PAIGU_CONFIG_PATH`（默认 `config/app.json`）、`PAIGU_HTTP_PORT`（默认 `21081`）。
- 成员名单读取顺序：`data/members.seed.json`（gitignored，真实名单，支持 `[...]` 或 `{"members":[...]}`）→ `data/members.example.json`（入库占位）→ 空数组；`/api/members` 的 `source` 为 `seed|example|empty`。路径可用 `PAIGU_MEMBERS_SEED_PATH` / `PAIGU_MEMBERS_EXAMPLE_PATH` 覆盖。

## 4. 商品目录（`round.items`，种子来自 `simulation-corpus/real-chat/月行水上`）

> ⚠️ **已废弃（2026-09-24）**：本节示例缺 `class`（现**自动推导**）、**调价**（`adjust_cents`）；且 **`box_size` 与 `variants[].pieces` 已移除**；**种类**为 `拼团/单领/整盒/特典`。见 [SPEC-UPDATE-2026-09-24.md](./SPEC-UPDATE-2026-09-24.md) §U6。

```jsonc
{ "item_id":"pass_sp", "name":"通行认证SP-月行水上", "kind":"split",
  "aliases":["通行证","通行认证SP","通行认证"],
  "variants":[
    {"variant_id":"v_jcl","name":"结城理","capacity":null,"aliases":[]},
    {"variant_id":"v_yy","name":"岳羽由加莉","capacity":null,"aliases":[]},
    {"variant_id":"v_ajs","name":"埃癸斯","capacity":null,"aliases":[]},
    {"variant_id":"v_hlw","name":"虎狼丸","capacity":null,"aliases":[]}
  ] }
```
另含：`hr_resume`(人事部简历SP-月行水上)、`fashion`(风尚速递SP-月行水上)、`gift_card`(特典卡组-校园凭证，变体含 `整套`，别名 `一套`)。

## 5. HTTP API（`127.0.0.1:21081`）

> ⚠️ **已废弃（2026-09-24）**：本表不全。现行接口见 [INTERFACES.md](./INTERFACES.md) §8，另新增 `/api/workflow`、`/api/rounds*`、`/api/items/suggest-aliases`、`/api/messages` CRUD、`/api/settlement/*`、`/api/replay`、`/api/snapshot/*`、`/api/events`。

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
| GET | `/api/members` | 成员列表（`data/members.seed.json` → `data/members.example.json` → 空） |
| POST | `/api/members/refresh` | 经 Gateway 拉取 `get_group_member_list` |
| GET | `/api/gateway/status` | WS 连接状态 |
| GET | `/api/replay`、`/api/replay/*` | 逐步重放（Wave 1–4 已实现；路由/返回值见 [INTERFACES.md](./INTERFACES.md) §8） |
| GET | `/` `/admin` `/sim` `/replay` | 静态页（display / admin / sim / replay） |
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
`/api/members` 读取顺序 seed → example → 空，`source` 为 `seed|example|empty`。

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
