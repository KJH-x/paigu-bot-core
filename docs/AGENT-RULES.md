# 子 Agent 协作规则（AGENT-RULES）

> **开工前必读**：[POLICY.md](./POLICY.md) → [DESIGN.md](./DESIGN.md) → [TASKS.md](./TASKS.md) → 本文件。
> 违反以下规则的改动会被主 agent 拒绝合并。

## 1. 仓库与边界

- 仓库根：`C:\_CustomPrograms\Pages\paigu-bot-core`。所有改动只在此目录内。
- **只改自己名下的文件**（DESIGN §2）。不得改 `src/engine/**`、`src/replay/**`、`src/parser/**`、`src/simulation/**` 的既有逻辑（需要时在 `docs/TASKS.md` 追加“待 A0 处理”）。
- 不得改 `main.rs`、`Cargo.toml`、各 `mod.rs`（A0 专属）。
- 不得删除既有文件；不得动 `simulation-corpus/**`（既有回归资产）。

## 2. 安全红线（最高优先）

- **绝不发送消息到真实群**：不得调用 `send_group_msg`/`send_msg`/`send_*`。`reply_enabled` 默认 `false`，实现时必须强制校验。
- NapCat 已接入真实群 `123456789`：只接收、只读拉取成员；测试一律走**本地模拟器/离线夹具**，不触发真实动作。
- 不得把密钥写入代码或提交：LLM key 只经 `api_key_env`（`DEEPSEEK_API_KEY`）读取。
- 隐私数据不入库：`config/app.json`、`data/**`、`*.xlsx`、`sample*.json`、`simulation-corpus/real-*/` 已被 `.gitignore` 忽略，保持忽略状态。
- 提交前运行 `npm run privacy`（`scripts/privacy-scan.mjs`）：命中真实昵称/密钥/内网 IP 或 `data/**`·`config/**`·`*.xlsx` 被跟踪即失败。

## 3. 编码约束

- 语言/依赖：Rust 2021 + 既有依赖；**新增依赖需在 `docs/TASKS.md` 说明并由 A0 加到 `Cargo.toml`**（已批准：`notify`）。
- 不加注释（除非必要）；不改既有业务逻辑；不引入未使用的抽象。
- 错误处理：`anyhow::Result`/既有 `AppError`；不 `unwrap()` 生产路径。
- 中文正确渲染（UTF-8）；日志用 `tracing`。
- 每个模块自带单测（`#[cfg(test)]`）。

## 4. 接口契约（冻结）

- HTTP API 路径/字段：严格按 DESIGN §5。
- 配置字段：严格按 DESIGN §3。
- OneBot 事件字段：`post_type/message_type/self_id/user_id/group_id/message_id/raw_message/message/sender`。
- 出站动作：`{ "action": "...", "params": {...}, "echo": "..." }`；响应 `{ "status": "ok|failed", "retcode": 0, "data": ..., "echo": "..." }`。
- 需要改契约时：先改文档，并在 `docs/TASKS.md` 记录，通知 A0。

## 5. 完成标准（DoD）

- `cargo build` 无 error；`cargo test` 全绿（含新增测试）。
- 既有回归不破坏：`simulation-corpus/real-samples`、`real-xlsx` 脚本仍 ALL PASS（A0 会跑）。
- 交付说明 ≤200 字：改了哪些文件、如何验证、遗留问题。

## 6. 沟通

- 遇阻塞/歧义：在 `docs/TASKS.md` 追加 `## 阻塞` 条目（谁、问题、需要的决定），并继续做不受阻的部分。
- 不与其他 agent 争夺文件；发现需要别人文件时，改为在 `docs/TASKS.md` 提“待办”。
