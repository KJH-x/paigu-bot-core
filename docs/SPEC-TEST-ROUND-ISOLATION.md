# SPEC · 测试轮隔离与批量测试消息（2026-10-13）

> 状态：**已实现（2026-10-13）**。口径已同步至 [INTERFACES.md](./INTERFACES.md) / [FUNCTIONAL.md](./FUNCTIONAL.md) / [DESIGN.md](./DESIGN.md) / [web/README.md](../web/README.md) / [AGENTS.md](../AGENTS.md) / [config.example.json](../config.example.json) / [tests/e2e/README.md](../tests/e2e/README.md)。
> 决策同步见 [DECISIONS.md](./DECISIONS.md) §B-4；工程动作见 [TODOS.md](./TODOS.md)「测试轮隔离（2026-10-13）」。

## 1. 背景与目标

- 轮次需要区分「测试轮 / 正常轮」：测试轮只服务测试群，正常轮只服务正常群。
- 伪造消息解包必须**隔离真实消息链路**，并且**只在测试群生效**。
- 新增**批量发送测试消息**端点，供脚本/回归灌入完整消息序列。
- webui 的**伪造发送（模拟器页）移除**，只保留**消息编辑**能力。

## 2. 最终口径（R1–R11）

| # | 口径 |
|---|---|
| R1 | `RoundSettings.category: "test" \| "normal"`（新增，serde 默认 `normal`，向后兼容旧轮次文件）。 |
| R2 | **测试轮只绑定测试群；正常轮只绑定正常群**（校验见 §4.2）。 |
| R3 | **"隔离"只作用于来自 NC 的消息**——即网关入口 `EventSink::handle`；这是 NC 与真实链路的唯一边界。 |
| R4 | 伪造解包条件：`gateway.accept_forged_messages` **且** `round.category == test` **且** `ev.group_id == gateway.test_group_id`；否则永不解包。 |
| R5 | 新增 `GatewayConfig.test_group_id: Option<String>`，指定测试群。 |
| R6 | 批量端点 `POST /api/sim/messages`：请求体允许**裸数组**，也允许 `{ "messages": [ … ] }`。 |
| R7 | `timestamp` **只在每条消息内**出现；批量层不定义 timestamp，也不定义 reset。 |
| R8 | 每条消息**逐条处理**（按数组顺序）；**不存在"整批拒绝"**。 |
| R9 | 缺失必需字段（`user_id` / `text` / `timestamp`）**直接 drop，不进入处理**；`group_id` 非测试群同样 drop。统一状态 **`Dropped`**（**取消**早期草案里的 `Skipped`）。 |
| R10 | 单条 `POST /api/sim/message` 与批量端点**均仅测试轮可用**（`category != test` → HTTP 400）。重置**仅手动**（`POST /api/sim/reset`）。 |
| R11 | webui **移除伪造发送并删除模拟器页**（`sim.html`/`sim.js`）；保留 **消息编辑**（`msglog.js`，display/index/settings 复用）。 |

## 3. 数据模型

```rust
// src/round/mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RoundCategory { #[default] Normal, Test }

// src/settings/mod.rs :: RoundSettings
#[serde(default)]
pub category: RoundCategory,

// src/settings/mod.rs :: GatewayConfig
#[serde(default)]
pub test_group_id: Option<String>,
```

注入项（`src/api/sim_routes.rs`，单条/批量共用；格式相比旧 `SimMessage` 有变化）：

```jsonc
{
  "user_id": "1001",            // 也接受别名 "id"；必需
  "text": "排 通行认证 克莱门莎 1", // 必需
  "timestamp": 1700000000000,   // 必需；整数ms / 纯数字串 / RFC3339 / 本地 "YYYY-MM-DD HH:MM[:SS]"
  "nickname": "成员01",          // 可选
  "is_admin": false,            // 可选，默认 false
  "message_id": "sim-1",        // 可选，缺省生成
  "group_id": "123456789"      // 可选，缺省 = test_group_id；≠ test_group_id 则该条 drop
}
```

- **移除**旧的 `offset_ms`（模拟器页已删，仅脚本可改；缺 `timestamp` 一律 drop）。
- 批量包装：

```jsonc
[ { … }, { … } ]                       // 裸数组
{ "messages": [ { … }, { … } ] }       // messages 包装
```

## 4. 代码改动（按文件）

### 4.1 领域 / 配置
- `src/round/mod.rs`：新增 `RoundCategory`（含 `as_str()`），并导出。
- `src/settings/mod.rs`：`RoundSettings.category`、`GatewayConfig.test_group_id`。
- `config.example.json`：`gateway.test_group_id: null`、`round.category: "normal"`。

### 4.2 绑定校验（`src/services/rounds.rs`）
- 新增 `fn category_issues(round: &RoundSettings, test_group_id: Option<&str>) -> Vec<Value>`：
  - `Test` ⇒ `test_group_id` 存在且 `== round.group_id`，否则 `error: test_round_group_mismatch`。
  - `Normal` ⇒ `test_group_id != Some(round.group_id)`，否则 `error: normal_round_uses_test_group`。
- 在 `check_round` 与 `setup_round` 的校验处追加（与现有白名单校验并列）。
- `SetupBody` / `CreateRoundBody` 增 `category: Option<RoundCategory>`；`merge_setup` 提供即替换、缺省保留；`copy_from` 沿用源轮次 category。

### 4.3 隔离（`src/llm/pipeline/mod.rs`）
- **删除** `process` 内的伪造解包分支（现 `L337-342`）。
- 新增纯函数（便于单测）：
  ```rust
  pub(crate) fn gate_nc(cfg: &AppConfig, ev: IncomingEvent) -> IncomingEvent {
      // accept_forged_messages && category==Test && group_id==test_group_id
  }
  ```
- NC 入口改为：
  ```rust
  impl EventSink for Pipeline {
      async fn handle(&self, ev: IncomingEvent) {
          let cfg = self.cfg.get().await;
          let ev = gate_nc(&cfg, ev);
          self.process(ev).await;
      }
  }
  ```
- `parse_timestamp_ms` 由私有改 `pub(crate)`，供端点复用。

### 4.4 测试注入端点（`src/api/sim_routes.rs`）
- 路由新增 `/api/sim/messages`（批量）；保留 `/api/sim/message`（单条）、`/api/sim/identity`、`/api/sim/reset`。
- 共用 `TestMessage`（§3）；批量体用 `#[serde(untagged)] enum BatchBody { List(Vec<TestMessage>), Wrapped { messages: Vec<TestMessage> } }`。
- 门槛：`cfg.round.category != Test` → `400 {"error":"not_a_test_round"}`（单条同）。
- 逐条：缺 `user_id`/`text`/`timestamp`（或 timestamp 解析失败）→ `Dropped`（detail 说明），**不调用 pipeline**；`group_id` 非测试群 → `Dropped`；否则构造 `IncomingEvent{ raw: None }` → `pipeline.process`。
- 返回见 §5。**无 reset 字段。**

### 4.5 前端移除（保留消息编辑）
- 删除 `web/sim.html`、`web/sim.js`。
- `src/api/mod.rs`：删 `sim_page`（`L89`）与 `.route("/sim", …)`（`L108`）。
- 移除导航：`admin.html:47`、`display.html:59`、`replay.html:66`、`round.html:52`、`settings.html:53`、`settlement.html:48`、`index.html:97-99`（nav-card）。
- `web/common.js`：删 `buildOneBotEvent`（`L506` 起）及导出（`L991`）。
- `web/shell.js`：`PAGE_DEFAULT` 删 `sim: 2`（`L27`）。
- 保留 `web/msglog.js` 及 `/api/messages` 增删改查。

## 5. API 契约

`POST /api/sim/messages`

```jsonc
// 请求：裸数组 或 {"messages":[...]}
// 响应：
{
  "ok": true,
  "round_id": "余音向海-恒远津梁-20261012",
  "category": "test",
  "count": 3,
  "summary": { "applied": 1, "dropped": 1, "rejected": 1, "ignored": 0 },
  "results": [
    { "index": 0, "user_id": "1001", "text": "…", "status": "Applied", "detail": "…", "version": 5 },
    { "index": 1, "user_id": "1002", "text": "…", "status": "Dropped", "detail": "missing_timestamp", "version": 5 }
  ],
  "board": { … }, "version": 5
}
```

- `400 {"error":"not_a_test_round"}`：活动轮非测试轮。
- `results[].status` 取 `Applied | Dropped | Rejected | Ignored | Duplicate`（沿用管线词表）。
- 单条 `POST /api/sim/message` 返回沿用 `{outcome, board, version}`，但同样加"仅测试轮"门槛。

## 6. 测试

### 6.1 Rust 单测
- `RoundCategory` 默认值 / 序列化往返。
- `category_issues`：test/normal × 群匹配/不匹配。
- `gate_nc`：开关 × 轮类 × 群的 8 组合。
- `BatchBody` 裸数组 / `messages` 包装解析。
- 端点逐条：缺 timestamp→Dropped、非测试群→Dropped、非测试轮→400。
- 通过 `cargo fmt` + `cargo clippy -- -D warnings` + `cargo test`。

### 6.2 e2e（改写 `tests/e2e/sim.mjs`）
- 移除对 `/sim` 页、`#send`、`/sim.js` 的依赖；改为 `POST /api/sim/messages` 注入。
- 保留 replay / settlement 等既有用例。
- **新增排版验收用例：Playwright 视口 `1920×1080`、`deviceScaleFactor = 1`（100% 缩放）**，逐页（`index` / `display` / `round` / `settings` / `settlement` / `replay` / `admin`）断言：
  - 无横向溢出（`document.documentElement.scrollWidth <= clientWidth + 1`）；
  - 关键面板/列不重叠、宽度非 0；
  - 全部导航链接可达（不含已删除的 `sim.html`）。
- 通过 `node tests/e2e/sim.mjs`。

## 7. 文档同步
- `config.example.json`、`docs/round-setup.template.json`（新增 `category`）。
- `docs/INTERFACES.md`：`GatewayConfig`/`RoundSettings` 字段；sim 端点表加 `/api/sim/messages`；伪造解包改为"仅测试轮+测试群的 NC 消息"。
- `docs/FUNCTIONAL.md`（模拟/测试节）、`docs/DESIGN.md`（sim 路由）、`docs/GAP-ANALYSIS.md`。
- `web/README.md`（删 sim 行）。
- `AGENTS.md` §4 红线：伪造解包仅测试群生效；模拟器页已移除。

## 8. 落地与验收

> 以下是2026-10-13当时的操作记录/方案，不是当前部署授权；后继工程仅隔离开发，未更新/重启常驻实例，不能照此对真实配置/群补做。当前绑定硬守卫与实际验收以INTERFACES §8.18及后继工程台账为准。
1. `.\deploy\stop.ps1` → `.\deploy\build.ps1` → `Start-ScheduledTask -TaskName paigu-bot-core-run`。
2. 经 `/api/rounds/:id/setup` 给 `余音向海-恒远津梁-20261012` 置 `category:"test"`；`config/app.json` 配 `test_group_id`。
3. 验收：批量灌 3 条（1 合法、1 缺 timestamp、1 非测试群）核对 `summary`/看板；普通群 NC 消息不解包；非测试轮调用返回 400；e2e（含 1920×1080/100% 排版）全 PASS。

## 9. 已答复的问（记录）

- **`Skipped` 是什么**：早期草案里，批量中"群号不属于测试群"的那条我不做处理，用一个单独的 `Skipped` 状态标记它。现按你的口径**取消 `Skipped`**：不处理即 `Dropped`，与管线既有词表一致。
- **"消息缺失直接 drop"**：缺 `user_id`/`text`/`timestamp`（或 timestamp 非法）的消息**不进入处理**，直接 `Dropped`。
- **时间缺失**：不再用 `now` 兜底；无 `timestamp` 即 drop。
