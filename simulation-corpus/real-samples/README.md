# real-samples —— 真实「事件重放」样本改造

来源（用户提供，位于仓库根目录）：

| 源文件 | 标题 | 内容 |
|---|---|---|
| `sample 1.json` | 月行水上 | 6 个 section：特典/特典卡组/通行认证SP/人事部简历SP/风尚速递SP/单领 |
| `sample2.json` | （无） | 4 个 section：通行认证53.0/风尚速递/特典/单领 |

两份样本是**排谷结束后的「事件重放」结果**，字段语义：

- `event_replay[].section`：分区/版块（商品组）
- `type`：`claim`（整组多份认领）/ `split_claim`（按变体认领）/ `direct_claim`（单领）
- `events[]`：`order`（认领顺序）、`cn`（昵称）、`variant`（变体）、`adjusted_price`/`adjustment`/`price`（调价，元）
- `package_tail`：某用户包走剩余变体（**包尾**）
- `unclaimed_variants`：无人认领的变体
- `ignored`：如 `quantity=0` 被忽略

## 改造映射（样本 → 本系统）

| 样本概念 | 本系统模型 |
|---|---|
| `section` | 拼进 `item.name` 前缀，同时写入 `metadata.section` |
| `claim` | 1 个 `Item{kind:split, box_size:total_quantity}`；每条 event → 一条 `排{name}` 消息 |
| `split_claim` + `variant` | 每个变体 1 个 `Item{kind:split, box_size:该变体认领次数}`；消息 `排{section}·{variant}` |
| `package_tail` | 对 tail 内每个变体发 `包尾{section}·{variant}`（`SlotPolicy::TailLocked`） |
| `direct_claim` | 每个商品 1 个 `Item{kind:single}`；消息 `排{section}·{item}` |
| `adjusted_price`/`adjustment`/`price` | 换算为 `unit_price_cents`（当前仅记录，不参与排位/结算） |
| `unclaimed_variants` | 生成 `Item` 但不生成消息（`metadata.unclaimed=true`） |
| `ignored` | 记入 `expected.json`，不生成消息 |

> 变体名跨 section 会重复（如「虎狼丸」出现在 4 个 section），故统一命名为 `{section}·{variant}` 以保证唯一；否则规则解析会命中多个商品。

## 产物

```
real-samples/
  convert.mjs               # 样本 → 夹具 转换器
  verify-samples.mjs        # 夹具结果 ↔ 样本原始事件顺序 比对器
  sample1/
    round_config.json       # 商品表（31 items）
    queue.jsonl             # 重建的群消息（71 条）
    expected.json           # 归一化样本（保留原始语义）
    expected_allocation.json# 期望分配（item_id → 有序 cn 列表 / 单领计数）
    out/                    # simulate 产物：report.md / result.json / outcomes.jsonl
  sample2/                  # 15 items / 28 messages
```

## 复现与验证

```bash
# 1) 生成夹具
node simulation-corpus/real-samples/convert.mjs . simulation-corpus/real-samples

# 2) 确定性重放
cargo run -- simulate \
  --round-config simulation-corpus/real-samples/sample1/round_config.json \
  --queue        simulation-corpus/real-samples/sample1/queue.jsonl \
  --out          simulation-corpus/real-samples/sample1/out

# 3) 与样本原始事件顺序逐项比对
node simulation-corpus/real-samples/verify-samples.mjs simulation-corpus/real-samples
```

## 验证结果

```
sample1: items=31 pass=31 fail=0
sample2: items=15 pass=15 fail=0
ALL PASS
```

即：**每个变体/整组/单领的最终占位与样本原始 `order→cn` 完全一致**，71/28 条消息全部生效。

## 与真实语义的差距（待决）

1. **变体建模**：当前把每个变体拆成独立 `Item`，靠命名区分；真实系统里变体应隶属于某个 base item（`metadata.base_item` 已保留，可供后续实现变体感知的分配）。
2. **包尾语义**：样本的 `package_tail` 是「一次性占走剩余变体」；当前引擎的 `TailLocked` 是盒槽级（开新盒占前 N 槽并锁其余），故改造为逐变体发 `包尾` 消息。若要原子化包尾，需要引擎支持「按 base item 锁定剩余变体」。
3. **价格/调价**：`adjusted_price`/`adjustment`/`original_price` 已换算进 `unit_price_cents`，但当前分配与结算未消费；真实系统需要按变体调价结算。
4. **claim vs split_claim 同 base item**：样本中同一 base item 先整组认领再按变体认领；改造中拆成两组 Item，未表达两阶段关系。
5. **`ignored`**：仅保留记录，未走「数量=0 → 拒绝」链路（该链路已在规则样本中验证）。
