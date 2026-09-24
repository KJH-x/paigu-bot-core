# AGENTS.md — paigu-bot-core 仓库指南

> 本文件是**仓库级 agent 入口**（含构建/测试/约定/红线）。文档总入口见 [docs/README.md](./docs/README.md)。

## 1. 项目定位

QQ 机器人拼团排谷系统：**本地做数据处理，远程做展示**。本机接收 QQ 群消息，经 LLM 清理/抽取与确定性引擎排谷计算；排位快照与回放将来发布到 Cloudflare（R2 + Pages）供成员查看（远程发布尚未接线）。

```text
NapCatQQ ──反向 WS──▶ gateway ──▶ llm::Pipeline ──▶ engine(replay+allocation)
                        │              │                    │
                        │              │                    ▼
                        │              │            AllocationSnapshot
                        ▼              ▼                    │
              (白名单/drop · 只读)  幂等→规则→LLM→校验→权限   ▼
                                                   api(axum) ──▶ web(静态页)
```

- 单进程：Gateway + Pipeline + HTTP API 同进程；`run` 不连接 PostgreSQL。
- 确定性优先：LLM 只做自然语言 → 结构化；排序/分配/结算由确定性引擎完成。
- 事件溯源：操作记录为不可变事件，状态由事件流重放得到。

## 2. 常用命令

```bash
cargo build                                  # 编译（无 error / 无新 warning）
cargo test                                   # Rust 单测（当前 139 passed）
node tests/e2e/sim.mjs                       # Playwright e2e（当前 11/11，脚本自带 cargo build）

# 回归语料（具体以 simulation-corpus/**/README.md 为准）
python simulation-corpus/real-xlsx/parse_xlsx.py          # ① xlsx → sections/messages JSON（需 *.xlsx）
python simulation-corpus/real-xlsx/build_fixtures.py      # ② 生成夹具 + 跑引擎 + 逐格比对（需先 cargo build）
node   simulation-corpus/real-samples/convert.mjs . simulation-corpus/real-samples   # ① 重建夹具
cargo run -- simulate --round-config simulation-corpus/real-samples/sample1/round_config.json \
                       --queue        simulation-corpus/real-samples/sample1/queue.jsonl \
                       --out          simulation-corpus/real-samples/sample1/out          # ② 生成 out/
node   simulation-corpus/real-samples/verify-samples.mjs simulation-corpus/real-samples  # ③ 先 simulate 再验证

# 脚本/隐私
npm run test:e2e        # = node tests/e2e/sim.mjs
npm run sim:run         # scripts/sim-run.mjs
npm run sim:record      # scripts/sim-record.mjs
npm run sim:verify      # = verify-samples.mjs simulation-corpus/real-samples
npm run privacy         # scripts/privacy-scan.mjs（跟踪文件隐私/密钥扫描）
```

## 3. 模块地图

| 路径 | 职责 |
|---|---|
| `src/main.rs` | 入口分派：`run`/`serve` → 新栈；`simulate` → 离线重放；其余按 `run` |
| `src/bus.rs` | 冻结接口：`IncomingEvent` / `EventSink` / `PipelineOutcome` |
| `src/settings.rs` | `AppConfig` + `ConfigStore`（revision + `notify` 热载） |
| `src/gateway/` | OneBot 反向 WS、白名单/drop 路由、心跳、echo 回包、只读动作、昵称清洗 |
| `src/llm/` | DeepSeek 客户端 + 排谷流水线（规则快路径 + LLM 清理/抽取 + 回退） |
| `src/api/` | axum 路由（config/board/display/message/replay/settlement/sim/member）+ 静态页 + `ApiError` |
| `src/domain/` | 领域模型：事件/快照/分配/认领/结算/金额/商品/用户/轮次 |
| `src/engine/` | 确定性分配引擎 + 事件存储 + 内存重放 |
| `src/parser/` | 规则解析、校验、归一化（`normalize.rs`，含 `clean_nickname`）、别名匹配、共享策略（`policy.rs`：白名单/优先时段/阶段）、回复文案 |
| `src/services/` | 服务层（`settlement`/`display`/`messages`/`members`/`replay`/`snapshot`）；API handler 仅做请求→服务→响应映射 |
| `src/planner/` | 下单表 planner（`GiftMax` / `DiscountMax`） |
| `src/replay/` | 逐步重放引擎 + `session` + `state_diff` |
| `src/round/` | 拼团阶段模型（时间窗 + 权限矩阵） |
| `src/settlement/` | 结算引擎 v2（减均 / 成团 / 特典） |
| `src/simulation/` | 离线 `simulate` verifier + 队列文件 |
| `src/snapshot_bundle/` | 单一 JSON 快照导出/导入 |
| `src/messages/` | 共享消息日志 `MessageLog`（JSONL + 原始事件） |
| `src/tests/` | Rust 重放辅助测试（`#[cfg(test)] mod tests;`） |
| `web/` | 静态前端：display / admin / sim / replay / settlement / **round（轮次与商品）/ settings（其余配置）** + `common.js`、`shell.js` |
| `data/rounds/` | **轮次库**（gitignored 运行时数据）：`<round_id>.json` = `RoundSettings`；`config/app.json` 的 `active_round_id` 指向激活轮次 |
| `tests/e2e/` | Node `.mjs` Playwright 端到端测试 |
| `scripts/` | 模拟/录制脚本 + `privacy-scan.mjs` |
| `config.example.json` | 配置模板（首次运行据此生成 `config/app.json`） |
| `data/members.example.json` | 占位成员（真实名单不入库） |
| `simulation-corpus/` | 回归语料（只读资产，不得修改） |
| `docs/` | 现行文档（入口 [docs/README.md](./docs/README.md)）+ `archive/` |

## 4. 约定

- **语言/版本**：Rust **2021**（`Cargo.toml` `edition = "2021"`）。
- **提交前**：`cargo fmt` + `cargo clippy`（并确保 `cargo build`/`cargo test` 通过）。
- **错误处理边界**：
  - 领域/引擎层：**当前无失败路径**（`settle`/`allocate`/`validate` 均不会 `Err`）；若将来引入可失败领域逻辑，用 `thiserror` 定义**具体**错误枚举（勿再建聚合型 `AppError`）；
  - 应用层：`anyhow`（`llm::pipeline`、`replay::session`、`messages`、`api`、`snapshot_bundle`、`simulation`）；
  - HTTP 层：`ApiError = (StatusCode, Json<Value>)`（`src/api/mod.rs`，`api_internal`/`api_bad_request`/`api_not_found`/`api_stale_revision` 映射）。
- **测试放置（统一约定，2026-09-23）**：按**测试模块行数**二选一：
  1. **≥ 120 行 → 目录化**：`src/foo.rs` → `src/foo/mod.rs`，测试放 `src/foo/tests.rs`，父文件写 `#[cfg(test)] mod tests;`（`mod foo;` 自动解析 `foo.rs` 或 `foo/mod.rs`，故改文件名无需动 `mod` 声明）。
  2. **< 120 行 → 内联** `#[cfg(test)] mod tests { … }`（同文件）。
  - 已目录化示例：`llm/pipeline/`、`planner/`、`settlement/`、`messages/`、`api/`、`round/`、`snapshot_bundle/`、`settings/`、`engine/{allocation_engine,settlement_engine}/`、`gateway/{onebot,ws_server}/`、`replay/session/`。
  - 独立测试入口：Node/Playwright `tests/e2e/sim.mjs`；Python 夹具 `simulation-corpus/**`。
- **业务要点（2026-09-24，SPEC-UPDATE U1–U9）**：
  - **轮次库**：`config/app.json` 只留 `active_round_id`；轮次数据 `data/rounds/<round_id>.json`；`GET/POST /api/rounds`、`POST /api/rounds/:id/activate`（`continue` 默认 / `fresh` / `replay`，**切换与重放解耦**）、`POST /api/rounds/:id/check`、`DELETE /api/rounds/:id`。开团=`/round`，设置=`/settings`（不进 Stepper）。
  - **成员 CN**：`members.cn_overrides[{user_id,cn,aliases}]`；人名回退 **CN → 归一化昵称 → user_id**；用于匹配与**结算表/账单人名**；`/api/members` 附 `cn/resolved`。
  - **调价在目录（仅变体）**：商品级只设原价；**仅变体** `原价 A ± 调价 B = 最终价 C`；结算页**不录入**调价、仅展示调后价；调价**不参与**折扣/减均/特典档位。⚠️ 现存差距：`VariantConfig.adjust_cents` 仅前端契约、后端未落库（见 [docs/TODOS.md](./docs/TODOS.md) W-G2-01）。
- **禁止事项（红线）**：
  - **绝不向真实群发消息**：`reply_enabled=false` 默认关闭；`send_*` 仅当开启且 `action ∈ allowed_actions` 时放行，否则强制拦截并告警。
  - 真实昵称 / 群号 / 配置 / `data/**` / `config/**` / `*.xlsx` **不得入库**；改动只提交占位名（如 `成员01`、`123456789`、`0.0.0.0:9801`）。
  - 不改 `simulation-corpus/**`（只读回归资产）；不改他人归属文件（见 [docs/MODULES.md](./docs/MODULES.md)）。

## 5. 隐私与安全检查

- `data/members.json`（真实成员缓存）与 `config/app.json`（本地配置）**均为 gitignored**，仅入库 `data/members.example.json` / `config.example.json`。
- 新增隐私扫描脚本：**`scripts/privacy-scan.mjs`**（`npm run privacy`）。它扫描全部 `git ls-files` 跟踪文件，命中即报错（退出码非 0）：
  - `data/members.json` 中的真实昵称（若文件存在；跳过占位昵称与通用短词）；
  - `config/app.json` 的本地值泄露到跟踪文件；
  - 常见密钥形态（`sk-` / `AKIA` / `ghp_` / `xox` / `BEGIN * PRIVATE KEY` / `Bearer <token>`）；
  - 内网 IP（`10/8`、`172.16/12`、`192.168/16`、`169.254/16`）；
  - `data/**`（除 `members.example.json`）、`config/**`、`*.xlsx` 被跟踪。
- 提交前请运行 `npm run privacy`；另外可用 `git grep` 复核真实昵称应为 **0 命中**。

## 6. 文档索引

- **总入口**：[docs/README.md](./docs/README.md)（LLM 接手工作入口，含必读顺序与红线）。
- ✅ **SPEC-UPDATE-2026-09-24 已实现（正文已更新）**：[docs/SPEC-UPDATE-2026-09-24.md](./docs/SPEC-UPDATE-2026-09-24.md)（U1–U9 现为现行口径；未落地差距见 [docs/TODOS.md](./docs/TODOS.md)「本轮遗留（Wave G2）」）。
- 现行：[POLICY.md](./docs/POLICY.md) / [DESIGN.md](./docs/DESIGN.md) / [REQUIREMENTS.md](./docs/REQUIREMENTS.md) / [INTERFACES.md](./docs/INTERFACES.md) / [FUNCTIONAL.md](./docs/FUNCTIONAL.md) / [MODULES.md](./docs/MODULES.md) / [TASKS.md](./docs/TASKS.md) / [AGENT-RULES.md](./docs/AGENT-RULES.md) / [GAP-ANALYSIS.md](./docs/GAP-ANALYSIS.md) / [DECISIONS.md](./docs/DECISIONS.md) / [TODOS.md](./docs/TODOS.md)。
- 归档：`docs/archive/README-legacy.md`（旧 README）。

## 7. 技术债（D-01…D-09 已于 2026-09-23 全部处理）

> 处置结果见 [docs/TODOS.md](./docs/TODOS.md) 「技术债（2026-09-23 审查 → 已全部处理）」。摘要：

| # | 状态 | 结果 |
|---|---|---|
| D-01 | ✅ | 结算真源统一为 `settlement::evaluate`（`SettlementEngine` 降为适配器） |
| D-02 | ✅ | 抽出 `src/parser/policy.rs`（白名单/优先时段/阶段拒绝），实时与重放共用 |
| D-03 | ✅ | 删除死重放栈（`ReplayService`/`rebuild_snapshot`） |
| D-04 | ✅ | 拆分 `llm/pipeline.rs` → `pipeline/{mod,state,llm_parse,admin,export,tests}.rs` |
| D-05 | ✅ | `clean_nickname` 下沉至 `src/parser/normalize.rs` |
| D-06 | ✅ | 删除 `error.rs`/`thiserror`；应用层统一 `anyhow` |
| D-07 | ✅ | 测试放置统一（见 §4 约定） |
| D-08 | ✅ | 删除 `MessageStore` trait；`MessageLog` 为唯一存储 API |
| D-09 | ✅ | 新增 `src/services/**`；handler 薄层化（HTTP 契约不变） |

> 仍开放的产品/工程项见 [docs/TODOS.md](./docs/TODOS.md)（T-14 管理员改单目标语法、T-19 拉取告警、T-24 `simulate` 迁移新栈、T-27 远程展示，以及 Wave G2 遗留 W-G2-05…07：`adjust_cents` 落库 / `suggest-aliases` 后端 / `box_size`·`pieces` 清理 / 重放 CN / `ColumnLocked` 语义等）。

## 8. 常驻运行 / 部署（`deploy/`）

长期运行实例由 Windows 计划任务 **`paigu-bot-core-run`**（登录时触发，用户 `NSLC`，Interactive/Limited）拉起 `deploy\run.ps1`。

| 命令 | 作用 |
|---|---|
| `.\deploy\build.ps1` | `cargo build --release` 并安装到 `deploy\bin\paigu-bot-core.exe`（**更新流程**） |
| `.\deploy\stop.ps1` | 停止计划任务实例并清理残留进程 |
| `Start-ScheduledTask -TaskName paigu-bot-core-run` | 启动/重启实例 |
| `Unregister-ScheduledTask -TaskName paigu-bot-core-run -Confirm:$false` | 取消登录自启 |

- `deploy\run.ps1`：单实例守卫 → `Set-Location` 仓库根（`config/`、`web/`、`data/` 为相对路径）→ 看护重启（5s→60s 退避）→ 日志 `deploy\logs\paigu-YYYYMMDD.log`（UTF-8、保留 14 天）。
- 说明见 [deploy/readme.md](./deploy/readme.md)。`deploy/bin/`、`deploy/logs/` 已 gitignore。
- **NapCat 不由本实例管理**：可延后启动；未连接时 bot 正常监听等待，连上即处理（`/api/gateway/status` 的 `clients`）。
