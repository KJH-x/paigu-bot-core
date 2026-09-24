# 排谷机器人 · 业务政策（POLICY）

> 本文件是**权威政策**。所有子 agent 必须阅读并遵守；实现与本文冲突时以本文为准。
> 相关：[DESIGN.md](./DESIGN.md)（程序路线）、[TASKS.md](./TASKS.md)（任务拆分）、[AGENT-RULES.md](./AGENT-RULES.md)（协作规则）。

## 0. 总原则

- **本地做数据处理，展示放远程**：本地（本机 `部署主机`）负责接收 QQ 群消息、LLM 解析、排谷计算；排位快照/回放发布到 Cloudflare（R2 + Pages）供成员查看。
- **绝不主动发消息给真实群**：NapCat 已接入真实群 `123456789`。当前阶段 **`reply_enabled=false`**：只接收、只计算、只记录；**不调用 `send_group_msg`**。成员名单拉取是只读操作，允许。
- **确定性优先**：LLM 只做自然语言→结构化；排序/分配/结算由确定性引擎完成。任何 LLM 输出都必须经校验层。

## 1. 接入与白名单（Gateway）

- 形态：**反向 WebSocket 服务器**，NapCat 主动连入 `ws://0.0.0.0:9801`。
- 鉴权：**不校验 token**（反向 WS 接受连接时不校验；无 token 配置项）。
- 只处理：`post_type == "message"` 且 `message_type == "group"` 且 `group_id ∈ whitelist_groups`。
- 白名单群：`["123456789"]`（可在 admin 面板热改）。
- 其余数据一律 **Drop**：非白名单群、私聊、非 message 事件（notice/meta_event/request）、空消息、无法解析帧。Drop 只记 debug 日志，不回复、不入库为业务事件。
- 出站动作：仅允许只读动作（当前只允许 `get_group_member_list`、`get_group_info`、`get_group_list`、`get_login_info`）。**默认禁止 `send_*`**：`send_*` 仅当 `reply_enabled=true` **且** `action ∈ allowed_actions` 时才放行（默认 `reply_enabled=false` → 绝不发送）。

## 2. 昵称清洗与身份

> ✅ **已更新（2026-09-24，见 [INTERFACES.md](./INTERFACES.md) §8.7）**：昵称清洗**单一真源已迁至 `src/parser/normalize.rs`**（`clean_nickname`，D-05）；并新增**成员具体名（CN）覆盖层**（绑定 `user_id`，用于匹配与结算表/账单人名，回退 `CN → 归一化昵称 → user_id`）。见 [SPEC-UPDATE-2026-09-24.md](./SPEC-UPDATE-2026-09-24.md) §U4。

- **去括号备注**：昵称中首个 `（` 或 `(` 起的内容去掉（全/半角），如
  `用户A（备注）→ 用户A`、`用户B（备注）→ 用户B`、`用户C/别名（备注）→ 用户C/别名`。
- **代理写法**：`A（代B）` / `A(代B)` → **身份 = `A`**，**显示 = `A(代B)`**（统一半角括号）。
  不同代理目标（`A(代B)` 与 `A(代C)`）**不合并**消费；who-whats 按显示串分组。
- **全角归一**：`：`→`:`、全角数字/字母→半角。
- **user_id 优先**：身份以 QQ `user_id` 为准；昵称仅用于展示。昵称↔user_id 映射来自群成员缓存（见 §7）。

> **实现单一真源（2026-09-24 更新）**：昵称清洗（全角归一 + 去括号备注 + 代理识别）只在 **`src/parser/normalize.rs` 的 `clean_nickname`** 实现；Gateway/Pipeline/校验层复用，不得另起一套。

**成员具体名（CN，U4）**：
- CN = **净化后的群昵称**，**绑定 QQ（`user_id`）**；存储 `members.cn_overrides[{user_id, cn, aliases[]}]`（config 热载 + 乐观并发 409）。
- **用途**：① 请求匹配；② **制作结算表格**（每人买了什么）时**自动使用 CN 作为人名**。
- **回退顺序：`CN → 归一化昵称（identity） → user_id`**（`settings::resolve_cn`）。
- 展示层 `/api/members` 每项附 `cn`（覆盖值，未配置为 `null`）与 `resolved`（回退结果）；`who_whats` 按成员缓存做 `nickname → CN` best-effort 覆盖。

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
7. **执行**：写事件 → 重放 → 排位快照 → （若 `reply_enabled=true` 且动作在白名单）回复 `已记录，当前版本 #N`。

**回复开关（已强制）**：`reply_enabled` 默认 `false`，此时**绝不发送任何消息**；置 `true` 后 `send_*` 仍须 `action ∈ allowed_actions` 才放行，否则拒绝并告警。回复与否不影响事件写入与快照更新。

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
- **预存用户（种子）**：`user_a, user_b, user_c, user_d`（admin 面板可热改；真实名单在 gitignored `data/members.seed.json`）。
- **排序**：预存用户全程 `priority_level=10`；排序键 `priority DESC → effective_at ASC → sequence ASC`（购物金优先排、非购物金延后排）。
- **时间基准**：政策按**消息 `timestamp_ms`** 判定（真实链路即 QQ 上报时间）。偏移仅用于模拟/测试（§6），不在真实链路。
- **管理员命令**：仅 `is_admin`（群主/管理员角色）可用；非管理员的 `/` 命令 → 拒绝/忽略。
- **回复开关与权限解耦**：`reply_enabled`（默认 `false`）只控制是否发送回复，且 `send_*` 仍受 `allowed_actions` 白名单约束（已强制）；权限政策只决定事件是否写入，与是否回复无关。

## 5. 排谷执行与数据

> ✅ **已更新（2026-09-24，见 [SPEC-UPDATE-2026-09-24.md](./SPEC-UPDATE-2026-09-24.md) §U6/U8）**：**种类**为 `拼团 / 单领 / 整盒 / 特典`。**整盒为独立种类**（默认进单领队列，可作独立单领条目）；`fullbox`(包盒) 保留为**拼团策略**；**包尾进拼团并在结算强制成盒**（按**列序=盒序号**锁定 + **自动滑入**）。

- **种类与 `class`**：`拼团`（有变体）/`特典`（有变体）⇒ `class=A`（阶段受限）；`单领`/`整盒`（无变体）⇒ `class=B`；`class` **自动推导**，显式 `class` 仅在合法时覆盖。
- 变体感知：`base 商品 + variants`；`split`（拼团，按变体/盒槽）/`single`（单领）/`wholebox`（整盒，进单领队列）/`gift`（特典）。
- 策略（`slot_policy`）：`normal` / `tail`(包尾) / `fullbox`(包盒)；`整盒` 由种类（非策略）表达。
- **列 = 盒的序号**；包尾锁定列 = 其申报变体集中各变体普通认购的**最大列序 + 1**；结算时锁定列**强制成盒**，不冲突（不在申报变体集内）的前序未成盒普通认购**自动滑入**。
- 幂等：同 `message_id` 不重复处理。
- 事件溯源 + 重放：所有操作记录为不可变事件，最终状态由重放得到；first-match 失败澄清结果以 `ParseOverride` 事件持久化并被重放消费（见 §3 流水线 / REQUIREMENTS §3.7）。
- 发布：排位快照 `current.json` + 回放 `steps/*.json` 发布到 R2（本地开发可写本地目录）。

## 6. 模拟器（仅用于重放/测试）

- 本地聊天页可：切换/新建/指定**真实群成员身份**；输入**时间偏移**（`realtime ± DD HH MM SS`）；发送任意消息。
- **时间偏移只改该消息的 `timestamp_ms`**（用于验证时段政策/重放），服务器“现在”不变。
- 真实链路**不包含**该功能。
- 模拟器走**与真实链路完全相同的流水线**（同一 LLM/规则/校验/权限/分配代码路径）。

## 7. 成员名单

- 群：`123456789`。
- **每日 19:00** 尝试 `get_group_member_list` 拉取，缓存到 `data/members.json`（gitignored）。
- 读取顺序：**刷新缓存 → `data/members.seed.json` → `data/members.example.json` → 空**（`/api/members` 的 `source` 为 `cache|seed|example|empty`；见 [DESIGN.md](./DESIGN.md) §8）。
- **只读**：只拉取，不发送任何消息。
- 提供手动“拉取成员”按钮（admin / settings 面板）。
- **成员具体名（CN，U4）**：`members.cn_overrides[{user_id, cn, aliases[]}]` 绑定 `user_id`；用于**匹配**与**结算表/账单人名**，回退 **CN → 归一化昵称 → user_id**；在 `/settings` 面板编辑（config 热载 + 乐观并发 409）。

## 8. 展示（成员可见）

- 内容：**排位表 + 消息流 + who-whats + 状态**。
- 刷新：**5s 增量轮询**（`since`/version），**只更新变化格**，不整页重渲染。
- **不打断交互**：不得打断划词（text selection）、点击、滚动；滚动仅在贴近底部时自动跟随，否则只显示“有新内容”计数（smart-scroll）。
- 数据源可切换：`local`（本机 API）/ `remote`（Cloudflare 上的静态快照）。远程为最终形态。

## 9. 配置热载

- 配置文件：`config/app.json`（gitignored），带 `revision`。
- 改动后**热载**（`notify` 文件监听；无依赖时退化为 mtime 轮询）。
- **prompt 即配置**：LLM 提示词、预存用户、时段、白名单、展示参数都在配置里，可编辑并保存（乐观并发：`revision` 不匹配 → 409 + 冲突提示）。
- **轮次库（U3，2026-09-24）**：`config/app.json` 仅保留 **`active_round_id`** 指向当前轮次；每个轮次一个文件 **`data/rounds/<round_id>.json`**（内容 = `RoundSettings`：round_id/title/group_id/priority_users/priority_window/phases/items）。`config/app.json` 的 `round` 仍是**运行时激活轮次**：启动/热载时若 `active_round_id` 指向的文件存在则据其覆盖 `round`，否则把当前 `round` 落盘并把 `active_round_id` 设为它（`PAIGU_ROUNDS_DIR` 可覆盖目录）。
- **切换与重放解耦**：`activate` 模式 `continue`（默认；无既有状态时等价 `fresh`）/ `fresh` / `replay`；重放是独立自动计算功能，不与切换绑定。轮次/商品等流程内配置在 `/round` 页编辑；其余不常改配置在 `/settings` 页（不进 Stepper）。
