# PLAN · 术语改名（含 JSON 键）+ 昵称/清洗/路由 + 前端 UX（2026-10-13）

> 决策：备份并**强制迁移**术语键、核心停机重启；接受新字段；成员 CN 落库=清洗后与用户自定义；「待确认」用自定义编辑器。
> 执行：详细计划 → 子代理审查/修正 → 写入本文件 → 子代理并发执行 + 验证改进 loop → 完成。

## 0. 备份与迁移流程
1. 停机：`.\deploy\stop.ps1`（核心 + 释放文件句柄）。
2. 备份：`Copy-Item data,... \Temp\opencode\paigu-backup-<ts>\`；`git bundle create`（全 refs）。
3. 迁移脚本：重写 `config/app.json`、`data/rounds/*.json`、`web/replay_view.json`、`docs/round-setup.template.json` 的键（见 §1）。
4. 迁移后 `cargo test` + 起服务 `Start-ScheduledTask`。

## 1. 术语改名（含 JSON 键），读写成对
映射：
| 旧 | 新 |
|---|---|
| 变体 | 子项 |
| variant / Variant（UI 文案） | subitem / Subitem |
| `variant_id` | `subitem_id` |
| `variants` | `subitems` |
| `variant_name` | `subitem_name` |
| `VariantConfig` | `SubitemConfig` |
| `ItemVariant` | `ItemSubitem` |
| `find_variant_by_id` / `variant_name_matches` / `is_bare_variant_name` | `find_subitem_by_id` / `subitem_name_matches` / `is_bare_subitem_name` |

serde（读旧兼容 + 写新）：
```rust
#[derive(Serialize, Deserialize)]
pub struct VariantConfig {           // → SubitemConfig
    #[serde(rename = "subitem_id", alias = "variant_id")]
    pub variant_id: String,          // → subitem_id
    ...
}
pub struct ItemConfig {
    #[serde(rename = "subitems", alias = "variants", default)]
    pub variants: Vec<VariantConfig>,  // → subitems
}
```
- 读写点：`settings/mod.rs`、`domain/item.rs`、`parser/{alias_match,rule_parser,parsed_event}`、`llm/pipeline/{llm_parse,mod}`、`llm/prompt.rs`、`services/{rounds,settlement,aliases}.rs`、`engine/allocation_engine`、`simulation/verifier.rs`、`web/{common,round,settlement,replay,display}.js`、`web/replay_view.json`、`docs/**`、`config.example.json`、`docs/round-setup.template.json`、所有测试。
- 保留的**语义枚举值**不动：`KINDS[].v`（group/single/box/gift）、`claim_type`（Split/Single/GiftClaim）、`slot_policy`。
- 迁移脚本对 `data/**`、`config/app.json` 就地重写键；`replay_view.json` 同改。

## 2. 金额格式化（元）
`web/common.js` 新增并导出：
```js
function fmtYuan(cents){ return (Math.round(Number(cents)||0)/100).toFixed(2) + ' 元'; }
function fmtYuan0(cents){ return (Math.round(Number(cents)||0)/100).toFixed(2); } // 输入框用，无“元”
```
- 展示点：`round.js:331/339/360/399/421-423/430`、`settlement.js:590-606/625-630/685-692/760-762` 用 `fmtYuan`；标签括号统一半角 `原价(元)`。
- **输入框 `value=` 不用 `fmtYuan`**（`round.js:360/399/421/423`、`settlement.js:192/215/218/269/275/417`）。

## 3. 昵称统一（消息流 == 排位表）
```rust
// llm/pipeline/mod.rs:427-432（live）
let display = crate::settings::resolve_cn(&cfg, &ev.user_id, &ev.nickname)
    .unwrap_or_else(|| ev.user_id.clone());
```
- `state::MessageRecord` 增 `user_id`；在 `mod.rs:443/759/1013` 与 `state.rs:105-115` 填充；`messages_since`（`state.rs:130-147`）输出 `user_id`。
- 保留代理注释 `发送者(代名称)`。`apply_replay` 已用 `resolve_cn`（`state.rs:100-115`），live 对齐后一致。

## 4. 昵称清洗（NFKC + 尾标点）
`Cargo.toml` 加 `unicode-normalization = "0.1"`。
```rust
// parser/normalize.rs
use unicode_normalization::UnicodeNormalization;
const TRAILING_PUNCT: &[char] = &['、','。','，','．','.','·','・',' ','　',':','：',';','；','!','！','?','？'];
pub fn clean_nickname(raw: &str) -> (String, String) {
    let nfkc: String = raw.nfkc().collect();
    let normalized = normalize_fullwidth(&nfkc);
    let t = normalized.trim();
    if let Some((a,b)) = split_proxy(t) { return (a.clone(), format!("{a}({b})")); }
    let mut identity = strip_remark(t);
    // 仅去“尾部”标点/空白（保留前导，如 "???"）
    while identity.chars().last().map_or(false, |c| TRAILING_PUNCT.contains(&c)) {
        identity.pop();
    }
    (identity.clone(), identity)
}
```
- 更新测试 `normalize.rs:126`（`乙.（备注乙）` → `乙`）；示例用占位名字（避免入库真实昵称）。
- 风险：identity 影响白名单/路由（属行为变更，评审接受）。

## 5. 整盒路由（不新增项）
- `ItemConfig` 增 `#[serde(default)] box_item_id: Option<String>`（拼团项→整盒项）。
- 解析后重定向（管线层，用 `cfg.round.items`，不动 domain `Item`）：
```rust
// 认领命中“拼团项”且子项为空 → 重定向到其 box_item_id（整盒）
if claim_variant_is_none && item.kind==Group && let Some(box_id)=item.box_item_id {
    claim.item_id = box_id; claim.kind = WholeBox;   // 整盒
}
```
- `通行证<子项>` / `<子项>` → 拼团（现状：前缀/子项绑定已成立）；`通行证` 单独 → 整盒。
- 目录校验：`pass55.box_item_id="pass55_box"`；不需要共享 token。

## 6. 成员表（settings 04）
- 列：`user_id | 原始名称 | 子项/昵称(清洗后, 可编辑) | CN | 别名 | 角色 | 操作`；**表头 sticky**。
- 清洗后列可编辑（默认自动清洗）；CN 未手改时**跟随**清洗列；保存写 `cn_overrides`（`{user_id, cn, aliases}`）。
- 筛选框：名称/CN/别名 + **拼音全拼/首字母**（多音字全组合）。
- 拼音来源：`members.rs::decorate_member` 附 `pinyin_full[]`/`pinyin_initials[]`；`buildMembers`（`settings.js:186-206`）透传；`renderMembers` 过滤。

## 7. 商品目录筛选 + 拼音
- `#kind-tabs` 行加搜索框（`round.html:132`）；`renderItems`（`round.js:306-324`）叠加名称/别名/子项 + 拼音过滤。
- 商品拼音：后端为 `ItemConfig`/`/api/config` 增算，或前端对 `state.items` 名称算拼音（同一 pinyin 能力）。

## 8. settings 布局
重写 `workbench.css:105-111`（保留 `#form>.layout{display:contents}`）：
```css
#sec-gateway{grid-column:1;grid-row:1}
#sec-display{grid-column:1;grid-row:2}
#sec-llm{grid-column:2;grid-row:1/3}
#sec-members{grid-column:1/-1;grid-row:3}
#sec-messages,#sec-snapshot{grid-column:1/-1}
```

## 9. round 页
- **单滚动（复用高度）**：把 `#sec-items .panel-body` 的 `max-height: calc(100vh - var(--shell-height) - 92px)`（`workbench.css:99`）**同样加到 `#sec-rounds > .panel-body`**；两列等高且都在视口内结束 → 无外层滚动。
- **sticky**：`#kind-tabs{position:sticky;top:0;z-index:3;background:opaque}`；`.cat-item-head{position:sticky;top:<kind-tabs高>;z-index:2;background:opaque}`（被下一 `.cat-item` 顶走）。
- **自定义模态**：新建轮次 / 当前激活轮次设置 / 阶段管理 → 模态（遮罩暗化 + 斜向宽排线底纹，`repeating-linear-gradient`）。迁移 DOM/JS。
- **cat-item-head 合并**：商品名移入 head；拼团项显示 `子项平均价(元)`（`avg = mean(unit+adjust)`）。
- **列宽**：`table.vtable th:last-child,td:last-child{width:44px}`；`商品别名` hint 补 `margin-inline`（对齐 `.cat-grid`/`.alias-wrap`）。

## 10. 「待确认」→ override 编辑器
- 消息流 `NeedConfirm` 行可点击 → 打开自定义模态：展示候选/原文，人工确认 → 写 `ParseOverride`（复用 `emit_parse_override`，需 `pub(crate)`）→ `recompute`。
- 新增 API：`POST /api/messages/:record_id/override {parsed|choice}`（或复用现有 first-match 澄清机制）。

## 11. 别名变更 → 刷新解析
- `put_config` 保存后：若 `round_id` 未变且商品名/别名/子项名 token 有变 → `state.pipeline.recompute()`（已确定性重解析全部消息）。
- 对已有 `ParseOverride` 的消息：异步重跑 LLM → 写新 `ParseOverride`（last-wins）。新增 `Pipeline::reparse_messages(records)`（包装 `emit_parse_override`）。

## 12. 执行顺序 / 并发 / 验证
1. 备份 + 迁移脚本（§1）。
2. 术语改名（含键）**单代理顺序**完成（跨文件）→ `cargo test`/clippy/e2e。
3. 并发子代理：WS-A 金额+清洗+昵称统一+user_id；WS-B settings/成员表+拼音+筛选；WS-C round 页（sticky/dialog/单滚动/head/列宽）；WS-D 整盒路由+别名刷新+待确认编辑器。
4. 每 WS 独立 `cargo test`/clippy；合并后全量 e2e。
5. 验证子代理审查 → 修正 loop → 完成。

## 13. 风险
- 术语改名跨 ~50 文件 + JSON 键：**必须成对**，遗漏读写任一侧即运行期失败；迁移脚本须覆盖 `data/**`、`config`、`replay_view.json`。
- `box_item_id` 重定向改引擎路径（拼团 variantless → 整盒 allocate_single），影响回归语料，需重基线。
- NFKC/尾标点改变 identity → 白名单/路由；`normalize.rs:126` 测试需改。
- `pinyin` crate 需联网拉取入库；离线不可得则前端 vendored。
- round 单滚动：确认 `#sec-rounds .panel-body` 复用高度后 `section-nav`/锚点仍可用（sticky 顶栏保留在 document）。

## 14. 子代理审查修正（2026-10-13）
- **§1 术语（NO-GO→修正）**：①`resolved_variant_id`→`resolved_subitem_id`（加 alias）；②**非 serde 读取点**必须**同时接受新旧键**或迁移：`simulation/verifier.rs:198-208`（语料**只读**，须兼容旧键）、`services/display.rs:104`、所有 `web/*.js`、`GET /api/rounds {variants}`（`services/rounds.rs:429`，消费于 `web/round.js:870`）、`data/settlement/*.json`（`SettlementDraft` 反序列化）；③补入清单文件：`domain/{claim,allocation}.rs`、`settlement/model.rs`、`planner/mod.rs`、`services/display.rs`、`parser/{normalize.rs:154,validation.rs:282}` 测试字面量；④LLM prompt 里 `variant` 键属**行为变更**。
- **§5 整盒路由（NO-GO→修正）**：重定向应在 **`parser/validation.rs:79-175`**（两分支产出 `Vec<(ItemId, Option<String>)>`），需给 `ValidateContext` 增 `box_map: HashMap<ItemId,ItemId>`（从 `mod.rs:621-632` 与 `replay/session/mod.rs:498-508` 传入）；设 `claim_type=Single`（WholeBox 走单领队列 `allocation_engine:112-113`）。原列的两处测试**实际不受影响**。let-chain 需改写（edition 2021）。
- **§7 拼音（NO-GO）**：`pinyin` crate 离线不可得 → **vendored 前端拼音库**；`#kind-tabs` 每次 `innerHTML` 重绘（`round.js:278-288`），搜索框须放 HTML 或重绘后追加。
- **§10 待确认编辑器（PARTIAL）**：消息流无 `record_id` → 需给 `state::MessageRecord`+`messages_since` 加 `record_id`（或改按 `seq`）；`emit_parse_override` 提升 `pub(crate)`。
- **§3 修正**：live `mod.rs:427` 现为 `clean_nickname`（**非** `resolve_cn`）；改为 `resolve_cn` 即行为变更，`who_whats`（`state.rs`/`services/display.rs:63-94`）随之统一。
- **§11 修正**：用 `services::replay::recompute(state)`（带 revision 守卫）；`put_config` 入口先读 `old cfg` 再 diff。
- **§9 注意**：`#sec-rounds .panel-body` 内已有 `.round-list{max-height:270px}` → 复用外层高度会产生嵌套滚动，需处理。
