"""月行水上-更新 夹具生成器。

输入：`月行水上-更新.xlsx`（用户提供，*.xlsx gitignored）
输出：`fixture.json` —— 下单表（12 单）、结算配置、Sheet2 参考价。

口径见 `docs/DECISIONS.md` §E：
- 忽略「折后价」列；不调价（用标价）
- 成几开几，不足整盒不成团（本次无包尾）
- 人事部简历SP 未成团 → 不买；特典 12 份 × ¥12 = ¥144
- 风尚速递SP = 拼 1 套 + 单领 2 份
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

try:
    import openpyxl
except ImportError:  # pragma: no cover
    print("需要 openpyxl：pip install openpyxl", file=sys.stderr)
    raise

ROOT = Path(__file__).resolve().parent
XLSX = ROOT / "月行水上-更新.xlsx"
OUT = ROOT / "fixture.json"

FIRST_ORDER_COL = 5  # E = 第1单
LAST_ORDER_COL = 16  # P = 第12单
ORDER_COUNT = LAST_ORDER_COL - FIRST_ORDER_COL + 1  # 12

# Sheet2 行：拼团/单领商品的 原价 与 每单分布
SHEET2_ITEM_ROWS = [2, 3, 4, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]
SHEET2_PER_ORDER_ROW = 24  # 每单价
SHEET2_TOTAL_ROW = 26  # 总价

# 额外（Sheet2 未含）：风尚速递SP 拼成 1 套（4 变体）= 1 份 ¥15 → 计入第1单
FASHION_SP_GROUP_UNIT_CENTS = 1500
FASHION_SP_GROUP_ORDER_INDEX = 0


def yuan_to_cents(value) -> int:
    if value is None or value == "":
        return 0
    return int(round(float(value) * 100))


def cell(ws, row: int, col: int):
    return ws.cell(row, col).value


def main() -> int:
    wb = openpyxl.load_workbook(XLSX, data_only=True)
    s1 = wb["Sheet1"]
    s2 = wb["Sheet2"]

    # ---- Sheet1：特典（两个 section） ----
    gift_sections = []
    # section1「特典（259元）」：A2 标题, B2 单价, C2 数量, D2.. cn
    if cell(s1, 2, 1):
        gift_sections.append(
            {
                "name": str(cell(s1, 3, 1)),
                "unit_price_cents": yuan_to_cents(cell(s1, 3, 2)),
                "count": int(cell(s1, 3, 3) or 0),
                "section": "特典（259元）",
            }
        )
    # section2「特典卡组-校园凭证」：拼成 1 套 = 1 份
    gift_sections.append(
        {
            "name": str(cell(s1, 5, 1)),
            "unit_price_cents": 1200,
            "count": 1,
            "section": "特典卡组-校园凭证",
        }
    )
    gift_claimed = sum(s["count"] for s in gift_sections)
    gift_unit = gift_sections[0]["unit_price_cents"]

    # ---- Sheet2：下单表（12 单） ----
    packages = [[] for _ in range(ORDER_COUNT)]
    for row in SHEET2_ITEM_ROWS:
        name = cell(s2, row, 1)
        if not name:
            continue
        price = yuan_to_cents(cell(s2, row, 2))
        if price == 0:
            continue
        for idx in range(ORDER_COUNT):
            qty = cell(s2, row, FIRST_ORDER_COL + idx)
            if qty in (None, "", 0):
                continue
            packages[idx].append(
                {
                    "item_id": str(name),
                    "variant_id": None,
                    "qty": int(qty),
                    "unit_price_cents": price,
                    "is_gift": False,
                }
            )

    # 风尚速递SP 拼成 1 套（Sheet2 未含）
    packages[FASHION_SP_GROUP_ORDER_INDEX].append(
        {
            "item_id": "风尚速递SP-月行水上（拼）",
            "variant_id": None,
            "qty": 1,
            "unit_price_cents": FASHION_SP_GROUP_UNIT_CENTS,
            "is_gift": False,
        }
    )

    order_table = {
        "packages": [
            {"package_id": f"第{i + 1}单", "lines": lines}
            for i, lines in enumerate(packages)
        ]
    }

    # ---- 参考价（Sheet2，仅验算） ----
    reference = {
        "per_order_cents": [
            yuan_to_cents(cell(s2, SHEET2_PER_ORDER_ROW, FIRST_ORDER_COL + i))
            for i in range(ORDER_COUNT)
        ],
        "total_cents": yuan_to_cents(cell(s2, SHEET2_TOTAL_ROW, 2)),
        "note": "Sheet2「单领（报盒）部分」为参考数据；不含特典价、不含风尚速递SP拼套(+¥15)。",
    }

    # ---- 结算配置 ----
    settlement_config = {
        "pricing": [],
        "discounts": [],
        "scope_mode": "ExcludeGift",
        "gift_tiers": [
            {
                "tier_id": "gift_card",
                "threshold": 0,
                "gift_name": "特典卡组-校园凭证",
                "unit_price": gift_unit,
                "claimed": gift_claimed,
            }
        ],
        "reduce_average": {"include_gift_price": False},
    }

    # ---- 期望值（口径自检） ----
    list_total = sum(
        l["unit_price_cents"] * l["qty"]
        for p in order_table["packages"]
        for l in p["lines"]
    )
    expected = {
        "P": ORDER_COUNT,
        "gift_claimed": gift_claimed,
        "gift_unit_cents": gift_unit,
        "G_cents": gift_claimed * gift_unit,
        "C_cents": list_total,
        "B_cents": list_total,  # 无折扣
        "D_cents": list_total - list_total + gift_claimed * gift_unit,
        "check": "Σfinal + G = B",
    }

    fixture = {
        "sample": "月行水上-更新",
        "source": "月行水上-更新.xlsx",
        "order_table": order_table,
        "settlement_config": settlement_config,
        "settlement": settlement_config,
        "reference": reference,
        "expected": expected,
        "gift_sections": gift_sections,
    }
    OUT.write_text(json.dumps(fixture, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"wrote {OUT}")
    print(
        "P={P} G={G} C={C} B={B} D={D}".format(
            P=expected["P"],
            G=expected["G_cents"],
            C=expected["C_cents"],
            B=expected["B_cents"],
            D=expected["D_cents"],
        )
    )
    print(f"gift_sections={gift_sections}")
    print(f"reference.total={reference['total_cents']} (Sheet2)")
    print(f"reference.per_order={reference['per_order_cents']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
