# 排谷机器人 · 程序设计路线（DESIGN）

> 与 [POLICY.md](./POLICY.md) 配套。本文件定义**模块、接口、数据结构、路由、部署**。
> 所有子 agent 必须遵守本文的**文件所有权**与**接口契约**。

## 1. 总体架构

```
                         (真实群 720675572)
 NapCatQQ ──reverse WS──▶ Gateway  192.168.100.2:9801
                              │  白名单/drop · 心跳 · 动作回包(只读)
                              ▼
                    Intake Queue (有界 mpsc)
                              ▼
   Pipeline:  幂等 → 规则快速路径 → LLM 清理/抽取 → 校验 → 权限 → 分配 → 回复(默认关闭)
                              │                         ▲
                              │                         │ Hot Config (config/app.json, revision)
                              ▼
        Engine/Replay(既有) → Snapshot → Publisher(R2/Local)
                              │
        axum HTTP API :21081 ─┼─ /api/config /api/board /api/display /api/messages
                              ├─ /api/sim/*  (模拟器，仅本地)
                              ├─ /api/members(/refresh)
                              └─ /web/* 静态页 (display/admin/sim)
                              ▼
        Cloudflare: R2(快照/回放) + Pages(静态展示页)
```

**运行模式**：本地一个进程同时跑 Gateway + HTTP API + Pipeline；`cargo run -- serve-all`（或默认 `run`）。
保留既有 `simulate`（离线重放验证）与 `serve`（旧聊天服务器）子命令，但新界面统一走 HTTP API。

## 2. 目录与文件所有权

| 路径 | 内容 | 归属 |
|---|---|---|
| `src/gateway/` | OneBot 协议、路由(白名单/drop)、WS server、动作回包 | **A1** |
| `src/llm/` | OpenAI 兼容客户端、排谷流水线 | **A2** |
| `src/config.rs` | `AppConfig` + 热载存储(revision) | **A2** |
| `src/api/` | axum 路由：config/board/display/messages/sim/members/replay | **A3** |
| `web/` | 静态页 display/admin/sim（vanilla，可部署） | **A4** |
| `tests/` | Node `.mjs` Playwright e2e + Rust 集成测试 | **A5** |
| `src/main.rs`、`src/app_state.rs`、各 `mod.rs` | 装配与接线 | **A0(主)** |
| `src/engine/**`、`src/replay/**`、`src/parser/**`、`src/simulation/**` | 既有引擎（改动需 A0 同意） | 主 |

> 子 agent **不得**改他人归属文件；需要跨模块改动时在 `docs/TASKS.md` 记录并等 A0 处理。

## 3. 配置（`config/app.json`，热载）

```jsonc
{
  "revision": 1,
  "gateway": {
    "bind": "192.168.100.2:9801",
    "require_token": false,
    "whitelist_groups": ["720675572"],
    "heartbeat_secs": 15,
    "reply_enabled": false,          // 真实群：禁止发消息
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
    "group_id": "720675572",
    "priority_users": ["kosame","SIM","芜笙","林恩克里斯蒂安"],
    "priority_window": { "start_ms": 1788782400000, "end_ms": 1788789600000 },
    "items": [ /* 见 §4 */ ]
  },
  "display": { "refresh_ms": 5000, "data_source": "local", "remote_base_url": "" },
  "members": { "group_id": "720675572", "cache_path": "data/members.json", "daily_pull_at": "19:00" }
}
```

- 存储：`ConfigStore`（`tokio::sync::RwLock<AppConfig>` + `revision`）；`PUT` 时校验 `revision`，成功 `revision+1`；`notify` 监听文件变更自动热载。
- 环境变量覆盖：`PAIGU_CONFIG_PATH`（默认 `config/app.json`）、`PAIGU_HTTP_PORT`（默认 `21081`）。

## 4. 商品目录（`round.items`，种子来自 `simulation-corpus/real-chat/月行水上`）

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
| GET | `/api/members` | 缓存的成员列表 |
| POST | `/api/members/refresh` | 经 Gateway 拉取 `get_group_member_list` |
| GET | `/api/gateway/status` | WS 连接状态 |
| GET | `/api/replay/*` | 既有回放桩，接线 |
| GET | `/` `/admin` `/sim` | 静态页 |

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
- 发布：每次快照变更后异步发布 R2（失败不阻塞业务）。

## 8. 成员子集（内置兜底，已按 POLICY §2 清洗）

```
澄猫三崎, 雨落, 霜星厨, KJH, SIM, 空格, Dele., HOA, 梓寒, 齐布/阿布, 晏, 以后当屯屯鼠,
Xnze, 聆听风声, 琉羽, cz, KitaKita, 羽翼青冥, Yomi, 双双, 竹璃, kosame, 二黑, 万事, 3206,
鱼见见, 少年, 終夏, 雪雉厨, umbb, 林苏, 不长談, 特别周, 幽烛黎夜, Malanda, 星砾, 林恩克里斯蒂安,
荷兰豆, 阿文AkameAya, code:015, wuchang, karie, LORD, 祁无争, ？？？, 嘟嘟, 枯枯, 可怜酱,
Kang, 芜笙, 韩江, 尤娜, 雾日, 楚狂, 稀饭, Nian, 稗子酒商, 千阳, 天江衣, 雀雀, 南极, 谌晨,
静边辰, 豆腐脑, 约德莱卡, 朔夜, 建安文容, goya, ソクサル
```
（原始含备注：`Dele.（凛冬）`、`齐布/阿布（俩都是我）`、`code：015` → 已清洗。）

## 9. 部署（将来）

- 本地：`cargo run`（Gateway + API + Pipeline），数据落 `data/`。
- 远程：`web/display` 静态页部署 Cloudflare Pages；快照/回放发布 R2；页面按 `display.data_source=remote` 读 `remote_base_url` 下 `rounds/{round_id}/current.json` 等。
- 展示页需支持两种数据源适配器（local/remote），同一套渲染。

## 10. 测试

- Rust：路由(白名单/drop)、配置 revision/热载、流水线(规则/LLM mock)、权限时段。
- Node `.mjs` Playwright：驱动 `/sim` 页面 → 选身份/设偏移/发消息 → 断言排位与状态；覆盖时段拒绝与预存优先。
- 回归：既有 `cargo test`（10）与 `simulation-corpus` 脚本必须保持通过。
