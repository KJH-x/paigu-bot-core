# 操作-时间表（2026-09-14 会话）

> 本文件为脱敏公开版：真实昵称以 `用户A` 代替；内部原版见 `CHANGELOG.md`（gitignored）。

时间 = 各文件 `LastWriteTime`（CST）。同一文件多阶段改动时只显示最终时间。

## 里程碑

| 时间 | 操作 | 产物 |
|---|---|---|
| 00:41 | 接入确定性规则解析器（`RuleOnly`） | `src/parser/rule_parser.rs`、`parser/mod.rs`、`normalize.rs` |
| 00:41–00:51 | `simulate` 验证器 + `serve` 本地聊天服务器 | `src/simulation/verifier.rs`、`chat_server.rs`、`src/main.rs` |
| 00:51 | 首轮验证报告（4 份 agent 语料，缺陷 D1–D8） | `simulation-corpus/VERIFICATION.md`、`LOGIC_CHAINS.md` |
| 01:05 | 改造 JSON 事件重放样本（变体/包尾/单领） | `simulation-corpus/real-samples/` |
| 01:28–01:29 | 引擎升级为**变体感知**（base 商品 + variants） | `domain/claim.rs`、`item.rs`、`allocation.rs`、`engine/allocation_engine.rs`、`engine/replay.rs`、`replay/replay_engine.rs` |
| 01:35–01:38 | xlsx 解析 + 夹具生成 + 逐格验证 + 查看器数据 | `real-xlsx/parse_xlsx.py`、`build_fixtures.py`、`build_replay_view.py`、`viewer/data/*` |
| 01:58 | 查看器启动脚本 | `viewer/serve.ps1` |
| 02:16–02:21 | 真实话术增强：变体别名 + 上下文绑定 + 相邻修饰合并 | `item.rs`、`verifier.rs`、`alias_match.rs`、`parsed_event.rs`、`validation.rs`、`rule_parser.rs` |
| 02:21–02:22 | 查看器新增**列视图** + 样本切换 | `viewer/index.html`、`viewer.js`、`style.css` |
| 02:22 | 真实群聊语料复现（14/14） | `real-chat/build_real_chat.py`、`real-chat/月行水上/*`、`viewer/data/真实聊天-月行水上.json` |
| 02:23 | 文档汇总 | `real-chat/README.md`、`viewer/README.md`、主 `README.md` |

## 文件级

| 时间 | 文件 | 操作 |
|---|---|---|
| 00:41:23 | `src/parser/mod.rs` | 新增 `rule_parser` 模块 |
| 00:41:25 | `src/parser/normalize.rs` | 增加「包盒/整盒/全包」归一 |
| 00:49:14 | `src/simulation/mod.rs` / `chat_server.rs` | 注册/新增聊天服务器 |
| 00:49:15 | `src/main.rs` | 增加 `simulate` / `serve` 子命令 |
| 00:51:26 | `simulation-corpus/VERIFICATION.md` | 首轮验证报告 |
| 00:51:36 | `LOGIC_CHAINS.md` | 追加确定性模拟链路 |
| 01:05:27 | `real-samples/convert.mjs` | 样本转换器 |
| 01:05:45 | `real-samples/verify-samples.mjs` | 逐项比对器 |
| 01:05:58 | `real-samples/README.md` | 样本说明 |
| 01:28:35 | `src/domain/claim.rs` | `SlotPolicy::FullBox`、`variant_id` |
| 01:28:36 | `src/domain/allocation.rs` | `ItemAllocation.variant_id` |
| 01:29:25 | `src/engine/allocation_engine.rs` | 分配键改 `(ItemId, variant)`、`FullBox`、包尾 clamp |
| 01:29:28 | `src/engine/replay.rs` / `replay/replay_engine.rs` | 变体 + version 回填 |
| 01:35:10 | `real-xlsx/parse_xlsx.py` | xlsx→规范 JSON |
| 01:36:42 | `real-xlsx/build_fixtures.py` | 生成 round_config/queue + 逐格比对 |
| 01:36:55 | `real-xlsx/build_replay_view.py` | 组装查看器数据 |
| 01:36:59 | `viewer/data/月行水上.json`、`覆雪于冬.json`、`replay_view.json` | 生成 |
| 01:38:52 | `real-xlsx/README.md` | 文档 |
| 01:58:49 | `viewer/serve.ps1` | 本地服务脚本 |
| 02:16:53 | `src/domain/item.rs` | `ItemVariant.aliases` |
| 02:16:58 | `src/simulation/verifier.rs` | 读取变体 aliases |
| 02:16:59 | `src/tests/replay_helpers.rs` | 适配新字段 |
| 02:17:06 | `src/parser/alias_match.rs` | 变体别名匹配 |
| 02:18:10 | `src/parser/parsed_event.rs` | `resolved_item_id/variant_id/round_id` |
| 02:18:16 | `src/parser/validation.rs` | 直接用已解析 hint，避免二次歧义 |
| 02:21:09 | `src/parser/rule_parser.rs` | 上下文绑定 + 相邻修饰合并 |
| 02:21:51 | `viewer/index.html` | 列视图 tab/容器 + 样本选项 |
| 02:21:57 | `viewer/viewer.js` | `renderColumn` + 视图切换 |
| 02:22:05 | `viewer/style.css` | 列视图样式 |
| 02:22:37 | `real-chat/build_real_chat.py`、`real-chat/月行水上/{round_config.json,queue.jsonl}`、`viewer/data/真实聊天-月行水上.json` | 真实语料复现产物 |
| 02:23:08 | `real-chat/README.md` | 语料 + md 评估 |
| 02:23:12 | `viewer/README.md` | 列视图文档 |
| 02:23:35 | `README.md` | 索引更新 |

## 追加（真实聊天政策 + 列视图对齐）

| 操作 | 文件 | 说明 |
|---|---|---|
| 引擎：优先时段政策 | `src/simulation/verifier.rs` | `RoundFixture` 增加 `priority_users` / `priority_window`；窗口内非预存用户 → Rejected；预存用户 `priority_level 10` |
| 语料：插入记录 + 昵称归一 + 政策 + who-whats | `simulation-corpus/real-chat/build_real_chat.py` | 新增 `09-07 21:29 用户A` 记录；`norm_person` 去括号备注；写 `priority_users/priority_window`；聚合 who-whats；被拒消息也占一步 |
| 记录表 / who-whats 生成脚本 | `real-chat/make_table.py`、`real-chat/show_who.py` | 从 queue+outcomes 生成 markdown 表 |
| 查看器：表格视图列对齐 | `viewer/style.css`、`viewer/index.html` | 固定列宽（label 150px、cell 72px）、不换行；`v=4` 破缓存 |
| 文档 | `real-chat/README.md`、`CHANGELOG.md` | 政策/记录表/who-whats 更新 |
