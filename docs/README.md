# 文档索引（docs/）

> 本目录是 paigu-bot-core 的**现行文档集合**。开始任何任务前，先读 [POLICY.md](./POLICY.md) → [DESIGN.md](./DESIGN.md) → [TASKS.md](./TASKS.md) → [AGENT-RULES.md](./AGENT-RULES.md)。

## 现行文档

| 文档 | 内容 | 读者 |
|---|---|---|
| [POLICY.md](./POLICY.md) | **权威业务政策**：接入/白名单、昵称清洗、消息判定流水线、权限时段、排谷数据、模拟器、成员名单、展示、配置热载 | 全体 |
| [DESIGN.md](./DESIGN.md) | 程序设计路线：总体架构、目录与文件所有权、配置、商品目录、HTTP API 路由、LLM 提示词、实时性、部署、测试 | 全体 |
| [TASKS.md](./TASKS.md) | 任务拆分（A0–A5）与文件所有权、完成状态、阻塞/待办 | 子 agent |
| [AGENT-RULES.md](./AGENT-RULES.md) | 子 agent 协作规则：边界、安全红线、编码约束、接口契约、DoD | 子 agent |
| [FUNCTIONAL.md](./FUNCTIONAL.md) | **功能描述**（面向评审）：当前代码实际实现的功能，触发 → 处理 → 结果 + JSON，含已知限制 | 用户/评审 |
| [MODULES.md](./MODULES.md) | 模块契约与文件所有权：`bus.rs`、`settings.rs`、`gateway/**`、`llm/**`、`api/**`、`web/**`、`tests/e2e/**`，及旧栈弃用清单 | 全体 |

## 归档

| 文档 | 说明 |
|---|---|
| [archive/README-legacy.md](./archive/README-legacy.md) | 旧版 README 归档（含旧栈定位/技术栈/结构；已删除过期端口与旧协议段） |

## 历史设计文档（根目录，gitignored，**勿引用**）

以下文件位于仓库根，已被 `.gitignore` 忽略，**不在现行文档体系内**，仅作本地历史参考：

- `ARCHITECTURE.md` - 旧完整系统架构与设计文档
- `LOGIC_CHAINS.md` - 旧全功能逻辑链条追踪
- `REPLAY_SIMULATION_ADDENDUM.md` - 旧事件重放/图形化审计/模拟排谷补充设计

> 现行架构与接口一律以 [DESIGN.md](./DESIGN.md) 与 [FUNCTIONAL.md](./FUNCTIONAL.md) 为准。

## 相关文档（仓库其他目录）

- [../README.md](../README.md) - 仓库总览与运行方式
- [../web/README.md](../web/README.md) - 前端（display / admin / sim / replay）
- [../tests/e2e/README.md](../tests/e2e/README.md) - 端到端测试说明
- 语料：`simulation-corpus/VERIFICATION.public.md`、`CHANGELOG.public.md`、`real-chat/README.public.md`（脱敏公开版）
