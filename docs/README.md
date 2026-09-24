# 文档索引 · LLM 接手工作入口（docs/）

> 本目录是 paigu-bot-core 的**现行文档集合**，也是 **LLM/子 agent 接手工作的唯一入口**。
> 任何新会话/新 agent 从本文件开始，按下面的顺序读完即可接手工作。

> ✅ **SPEC-UPDATE-2026-09-24 已实现（正文已更新）**：[SPEC-UPDATE-2026-09-24.md](./SPEC-UPDATE-2026-09-24.md) 的用户口径 U1–U9 已为**现行口径**，并同步进 [REQUIREMENTS.md](./REQUIREMENTS.md) / [POLICY.md](./POLICY.md) / [DESIGN.md](./DESIGN.md) / [INTERFACES.md](./INTERFACES.md)（原 `⚠️ 已废弃` 标记已改为 `✅ 已更新`）。未落地的实现差距记为 [TODOS.md](./TODOS.md)「本轮遗留（Wave G2）」。

## 一、必读顺序（接手工作前）

| 序 | 文档 | 内容 | 必读 |
|---|---|---|---|
| 1 | [POLICY.md](./POLICY.md) | **权威业务政策**：接入/白名单、昵称清洗、LLM 流水线、权限时段、排谷、成员、展示、热载 | ✅ 全体 |
| 2 | [DESIGN.md](./DESIGN.md) | **程序设计路线**：架构、目录与文件所有权、配置、商品目录、HTTP API、实时性、部署 | ✅ 全体 |
| 3 | [REQUIREMENTS.md](./REQUIREMENTS.md) | **功能需求**：检查清单 C1–C7、拼团阶段模型、结算/下单表规则、待定稿公式 | ✅ 全体 |
| 4 | [INTERFACES.md](./INTERFACES.md) | **接口映射表**：模块/接口/数据契约/需求→接口映射/解耦约束 | ✅ 全体 |
| 5 | [FUNCTIONAL.md](./FUNCTIONAL.md) | **功能描述**（用户评审主文档）：当前实现的功能（触发→处理→结果 + JSON） | 评审/改功能时 |
| 6 | [MODULES.md](./MODULES.md) | 模块契约与文件所有权 | ✅ 全体 |
| 7 | [TASKS.md](./TASKS.md) | 任务拆分（A0–A5、T1–T7）与完成状态 | 领任务时 |
| 8 | [GAP-ANALYSIS.md](./GAP-ANALYSIS.md) | **差距分析**：现状 vs 需求、任务拆分、Wave 完成状态 | ✅ 全体 |
| 9 | [AGENT-RULES.md](./AGENT-RULES.md) | 协作规则（边界/红线/接口/DoD） | ✅ 全体 |
| 10 | [DECISIONS.md](./DECISIONS.md) | **待决策 + 需补充信息**（含建议默认） | 领任务时 |
| 11 | [TODOS.md](./TODOS.md) | **工程待办**（ID/优先级/依赖/验收） | 领任务时 |

> **流程两页（2026-09-24）**：**开团 = `/round`**（轮次与商品；轮次库 `active_round_id` + `data/rounds/<round_id>.json`，切换与重放解耦），**其余配置 = `/settings`**（不进 Stepper）；详见 [DESIGN.md](./DESIGN.md) §3/§5、[POLICY.md](./POLICY.md) §9、[INTERFACES.md](./INTERFACES.md) §8.7。

## 二、LLM 接手工作协议

1. **领任务**：打开 [TODOS.md](./TODOS.md)，按优先级选一条 `todo`；确认其 `依赖`（D-xx）已在 [DECISIONS.md](./DECISIONS.md) 决策或可用「默认」实现。
2. **读契约**：按「必读顺序」读完 1–6，再看任务涉及的源码文件。
3. **改文件**：严格遵守 [MODULES.md](./MODULES.md) 的**文件所有权**；不确定时在 TODOS 该条目追加「待确认」而不是越界修改。
4. **实现与测试**：遵守 [AGENT-RULES.md](./AGENT-RULES.md)（红线：绝不向真实群发消息；真实昵称/隐私不入库）。
5. **验收**：
   - `cargo build` 无 error/新 warning；
   - `cargo test` 全绿；
   - `node tests/e2e/sim.mjs` 全 PASS；
   - 回归：`real-samples`、`real-xlsx` 脚本 ALL PASS。
6. **交付**：
   - 更新 [TODOS.md](./TODOS.md) 该条目状态（todo→done）+ 填「验收证据」；
   - 若产生新决策项 → 追加到 [DECISIONS.md](./DECISIONS.md)；
   - 若功能行为变化 → 同步 [FUNCTIONAL.md](./FUNCTIONAL.md) 与 [INTERFACES.md](./INTERFACES.md)；
   - 运行 `git status` 确认干净后提交（提交信息 `feat(scope): ...`）。

## 三、红线（不可违反）

- **绝不向真实群发送消息**（群 `123456789`；`reply_enabled=false`；`send_*` 被引擎强制拦截）。
- **真实昵称/隐私不入库**：69 人名单与优先昵称不得出现在任何跟踪文件（`data/members.seed.json` 由用户放置、gitignored；入库仅 `data/members.example.json` 占位）。
- 不改 `docs/REQUIREMENTS.md` 的业务口径（只可标注「待确认」）；实现与需求冲突时以需求为准并上报。

## 四、当前状态快照

- 详见 [GAP-ANALYSIS.md](./GAP-ANALYSIS.md)（完成状态）与 [TODOS.md](./TODOS.md)（待办）。
- 快速事实：`cargo test` 139 passed · `cargo build` 0 warning · e2e 11/11 · real-samples/real-xlsx 逐格 ALL PASS · `npm run privacy` OK（真实昵称跟踪文件 0 命中）。

## 五、归档 / 历史

| 文档 | 说明 |
|---|---|
| [archive/README-legacy.md](./archive/README-legacy.md) | 旧 README 存档（旧栈端口/旧协议/旧结构），已删除其现行段 |

> 根目录 `ARCHITECTURE.md` / `LOGIC_CHAINS.md` / `REPLAY_SIMULATION_ADDENDUM.md` 为**历史文档**（gitignored，勿引用）；现行架构以 [DESIGN.md](./DESIGN.md)、功能以 [FUNCTIONAL.md](./FUNCTIONAL.md) 为准。

## 六、仓库内其它文档

- [../AGENTS.md](../AGENTS.md) - 仓库级 agent 指南（模块地图 / 常用命令 / 约定 / 红线 / 隐私扫描）
- [../README.md](../README.md) - 仓库总览与运行方式
- [../web/README.md](../web/README.md) - 前端（display / admin / sim / replay / settlement）
- [../tests/e2e/README.md](../tests/e2e/README.md) - 端到端测试说明
- [../simulation-corpus/](../simulation-corpus/) - 语料与回归（`*.public.md` 为可入库脱敏版）
