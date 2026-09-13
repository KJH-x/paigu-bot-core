# 排谷机器人 · 业务政策（POLICY）

> 本文件是**权威政策**。所有子 agent 必须阅读并遵守；实现与本文冲突时以本文为准。
> 相关：[DESIGN.md](./DESIGN.md)（程序路线）、[TASKS.md](./TASKS.md)（任务拆分）、[AGENT-RULES.md](./AGENT-RULES.md)（协作规则）。

## 0. 总原则

- **本地做数据处理，展示放远程**：本地（本机 `192.168.100.2`）负责接收 QQ 群消息、LLM 解析、排谷计算；排位快照/回放发布到 Cloudflare（R2 + Pages）供成员查看。
- **绝不主动发消息给真实群**：NapCat 已接入真实群 `720675572`。当前阶段 **`reply_enabled=false`**：只接收、只计算、只记录；**不调用 `send_group_msg`**。成员名单拉取是只读操作，允许。
- **确定性优先**：LLM 只做自然语言→结构化；排序/分配/结算由确定性引擎完成。任何 LLM 输出都必须经校验层。

## 1. 接入与白名单（Gateway）

- 形态：**反向 WebSocket 服务器**，NapCat 主动连入 `ws://192.168.100.2:9801`。
- 鉴权：**不校验 token**（`require_token=false`）。
- 只处理：`post_type == "message"` 且 `message_type == "group"` 且 `group_id ∈ whitelist_groups`。
- 白名单群：`["720675572"]`（可在 admin 面板热改）。
- 其余数据一律 **Drop**：非白名单群、私聊、非 message 事件（notice/meta_event/request）、空消息、无法解析帧。Drop 只记 debug 日志，不回复、不入库为业务事件。
- 出站动作：仅允许只读动作（当前只允许 `get_group_member_list`、`get_group_info`、`get_group_list`、`get_login_info`）。**禁止 `send_*` 类动作**（由 `reply_enabled=false` 强制）。

## 2. 昵称清洗与身份

- **去括号备注**：昵称中首个 `（` 或 `(` 起的内容去掉（全/半角），如
  `SIM（良乡囤货）→ SIM`、`Dele.（凛冬）→ Dele.`、`齐布/阿布（俩都是我）→ 齐布/阿布`、`芜笙（阴暗潜水king👀）→ 芜笙`。
- **代理写法**：`A（代B）` / `A(代B)` → **身份 = `A`**，**显示 = `A(代B)`**（统一半角括号）。
  不同代理目标（`A(代B)` 与 `A(代C)`）**不合并**消费；who-whats 按显示串分组。
- **全角归一**：`：`→`:`、全角数字/字母→半角。
- **user_id 优先**：身份以 QQ `user_id` 为准；昵称仅用于展示。昵称↔user_id 映射来自群成员缓存（见 §7）。

## 3. 消息判定流水线（LLM-first + 规则兜底）

顺序：
1. **接入路由**（§1）：白名单 + 群消息 + 非空，否则 Drop。
2. **幂等**：`(group_id, message_id)` 已处理 → 忽略。
3. **规则快速路径**：`RuleParser` 能高置信度解析为 排谷/撤销/管理员命令 → 直接进入校验。
4. **LLM 清理与过滤**：判断是否为排谷消息并抽取结构化结果（商品/变体/数量/单领/包盒/包尾/代牌/撤销）。
   - 非排谷（闲聊、表情、图片、@、无关）→ **Ignore**（不回复）。
   - 商品歧义 → **NeedConfirm**。
   - 商品未找到 → **Reject**（提示可用商品）。
5. **校验**：置信度阈值、数量（0 拒绝、>99 拒绝）、别名唯一匹配。
6. **权限**（§4）。
7. **执行**：写事件 → 重放 → 排位快照 → （若 `reply_enabled`）回复 `已记录，当前版本 #N`。

LLM 失败/超时：`fallback_to_rules=true` 时回退规则解析器；仍失败 → 回复“没识别成功”。

LLM 输出契约（严格 JSON）：
```json
{ "intent": "claim|cancel|admin|unknown",
  "items": [ { "name": "结城理", "variant": "结城理", "item": "通行认证SP-月行水上",
               "quantity": 1, "claim_type": "split|single", "slot_policy": "normal|tail|fullbox" } ],
  "confidence": 0.0, "ambiguous_parts": [] }
```
- 不允许 LLM 计算价格/排序；只抽取。
- `variant` 与 `item` 二选一或同时给（用于消歧），最终由 `alias_match` 决定。

## 4. 权限政策（时段 + 预存）

- **优先时段**：`[2026-09-07 20:00, 22:00)`（**end 独占**）内，**仅预存(购物金)用户**可排；其余请求 **Reject**。
- **全量时段**：`2026-09-07 22:00` 起，所有人可排。
- **预存用户（种子）**：`kosame, SIM, 芜笙, 林恩克里斯蒂安`（admin 面板可热改）。
- **排序**：预存用户全程 `priority_level=10`；排序键 `priority DESC → effective_at ASC → sequence ASC`（购物金优先排、非购物金延后排）。
- **时间基准**：政策按**消息 `timestamp_ms`** 判定（真实链路即 QQ 上报时间）。偏移仅用于模拟/测试（§6），不在真实链路。
- **管理员命令**：仅 `is_admin`（群主/管理员角色）可用；非管理员的 `/` 命令 → 拒绝/忽略。

## 5. 排谷执行与数据

- 变体感知：`base 商品 + variants`；`split`（拼团，按变体/盒槽）/`single`（单领）/`gift`。
- 策略：`normal` / `tail`(包尾) / `fullbox`(包盒)。
- 幂等：同 `message_id` 不重复处理。
- 事件溯源 + 重放：所有操作记录为不可变事件，最终状态由重放得到。
- 发布：排位快照 `current.json` + 回放 `steps/*.json` 发布到 R2（本地开发可写本地目录）。

## 6. 模拟器（仅用于重放/测试）

- 本地聊天页可：切换/新建/指定**真实群成员身份**；输入**时间偏移**（`realtime ± DD HH MM SS`）；发送任意消息。
- **时间偏移只改该消息的 `timestamp_ms`**（用于验证时段政策/重放），服务器“现在”不变。
- 真实链路**不包含**该功能。
- 模拟器走**与真实链路完全相同的流水线**（同一 LLM/规则/校验/权限/分配代码路径）。

## 7. 成员名单

- 群：`720675572`。
- **每日 19:00** 尝试 `get_group_member_list` 拉取，缓存到 `data/members.json`（gitignored）。
- 拉取失败或未接入 → 使用内置子集（见 [DESIGN.md](./DESIGN.md) §8，已按 §2 清洗）。
- **只读**：只拉取，不发送任何消息。
- 提供手动“拉取成员”按钮（admin 面板）。

## 8. 展示（成员可见）

- 内容：**排位表 + 消息流 + who-whats + 状态**。
- 刷新：**5s 增量轮询**（`since`/version），**只更新变化格**，不整页重渲染。
- **不打断交互**：不得打断划词（text selection）、点击、滚动；滚动仅在贴近底部时自动跟随，否则只显示“有新内容”计数（smart-scroll）。
- 数据源可切换：`local`（本机 API）/ `remote`（Cloudflare 上的静态快照）。远程为最终形态。

## 9. 配置热载

- 配置文件：`config/app.json`（gitignored），带 `revision`。
- 改动后**热载**（`notify` 文件监听；无依赖时退化为 mtime 轮询）。
- **prompt 即配置**：LLM 提示词、商品目录、预存用户、时段、白名单、展示参数都在配置里，可在 admin 面板编辑并保存（乐观并发：`revision` 不匹配 → 409 + 冲突提示）。
