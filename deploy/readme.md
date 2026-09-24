# deploy —— 长期运行实例（登录自启）

## 目的
把排谷机器人作为常驻进程运行：接收 NapCat 反向 WS 消息、处理、并对外提供只读展示/API。
**不向真实群发送消息**（`config/app.json` 的 `reply_enabled=false`；`send_*` 被引擎强制拦截）。

## 入口
- `run.ps1` —— **唯一入口**（计划任务「登录时」触发）。行为：
  1. 单实例守卫（若已有 `paigu-bot-core` 进程则直接退出）；
  2. `Set-Location` 到仓库根（`config/`、`web/`、`data/` 均为相对路径，依赖 CWD）；
  3. 看护循环启动 `bin\paigu-bot-core.exe run`，stdout+stderr 追加到 `logs\paigu-YYYYMMDD.log`；
  4. 进程退出后按退避（5s→60s）重启；
  5. 启动时清理 > 14 天的日志。
- `build.ps1` —— `cargo build --release` 并把 exe 复制到 `bin\`（**更新流程**）。
- `stop.ps1` —— 停止计划任务实例并清理残留进程。

## 运行时要求
- Windows + PowerShell；Rust 工具链仅在**构建**时需要（运行只要 `bin\paigu-bot-core.exe`）。
- 环境变量：`DEEPSEEK_API_KEY`（用户级，LLM 需要）；`RUST_LOG` 可选（默认 `info`）。
- 网络：Gateway 反向 WS 监听 `部署主机网卡:9801（示例 `0.0.0.0:9801`）`（NapCat 主动连入）；HTTP API `127.0.0.1:21081`。
- **NapCat 不由本实例管理**：可能延后启动；未连接时机器人正常监听等待，连上即开始处理。

## 计划
- 触发：**AtLogOn**（当前用户，Interactive / Limited）。
- 任务名：`paigu-bot-core-run`。
- 设置：不限时（`ExecutionTimeLimit=0`）、`MultipleInstances=IgnoreNew`、失败重启（`RestartCount=999` / 1min）。

## 输入 / 输出
- 输入：NapCat 反向 WS 事件（`config/app.json` 白名单群/成员）。
- 输出：`data/messages/<round>.jsonl`、`data/events/<round>.jsonl`、`data/members.json`（缓存）、
  `data/snapshots/*.snapshot.json`（`/导出` 或 `/api/snapshot/export`）、`logs/paigu-*.log`（运行时日志）。
- HTTP：`/api/health`、`/api/gateway/status`、`/api/board`、`/api/display`、`/api/messages`、`/api/replay`、
  `/api/settlement/*`、`/api/members`、`/api/snapshot/*`、`/api/events`；静态页 `/`、`/admin`、`/sim`、`/replay`、`/settlement`。

## 失败 / 恢复
- 进程崩溃 → 看护循环 ≤60s 重启；HTTP 21081 被占用会退出并由看护重启（gateway 9801 自身有 2s 重试绑定）。
- NapCat 断线 → 自动重连（实测约 5s）；`/api/gateway/status` 的 `clients` 反映连接数。
- 手工停止：`deploy\stop.ps1`（或 `Stop-ScheduledTask -TaskName paigu-bot-core-run`）。
- 回滚自启：`Unregister-ScheduledTask -TaskName paigu-bot-core-run -Confirm:$false`。

## 更新流程
```powershell
.\deploy\build.ps1        # 构建 + 安装到 bin\
.\deploy\stop.ps1         # 停止
Start-ScheduledTask -TaskName paigu-bot-core-run   # 重新拉起
```

## 不入库
`deploy/bin/`（二进制）与 `deploy/logs/`（日志）已 gitignore。
