# -*- coding: utf-8 -*-
"""Build round_config/queue from sections.json+messages.json, run the engine, and
compare the engine allocation against the xlsx cell-by-cell order."""
import json
import os
import subprocess
import sys

ROOT = r"C:\_CustomPrograms\AnAgent\workspace\paigu-bot-core-20260510"
OUT = os.path.join(ROOT, "simulation-corpus", "real-xlsx")
EXE = os.path.join(ROOT, "target", "debug", "paigu-bot-core.exe")
SAMPLES = [
    {"slug": "月行水上", "group": "30001"},
    {"slug": "覆雪于冬", "group": "30002"},
]


def build(slug, group):
    d = os.path.join(OUT, slug)
    sec = json.load(open(os.path.join(d, "sections.json"), encoding="utf-8"))
    msgs = json.load(open(os.path.join(d, "messages.json"), encoding="utf-8"))

    items = []
    expected = {}
    tech_map = {}
    # map (section,row) -> (item_id, variant_id, technical_name, display_name)
    tech = {}
    for si, s in enumerate(sec["sections"]):
        kind = s["kind"]
        if kind == "split_variants":
            base_id = f"{slug}_v{si}"
            items.append(
                {
                    "item_id": base_id,
                    "name": f"__base_{si}",
                    "kind": "split",
                    "unit_price_cents": 0,
                    "box_size": None,
                    "max_quantity": None,
                    "aliases": [],
                    "variants": [],
                }
            )
            base = items[-1]
            for ri, row in enumerate(s["items"]):
                vid = f"{base_id}_r{ri}"
                cap = max(1, len(row["claims"]))
                tname = f"{row['name']}（S{si}）"
                base["variants"].append(
                    {
                        "variant_id": vid,
                        "name": tname,
                        "unit_price_cents": row.get("price_cents") or 0,
                        "capacity": cap,
                    }
                )
                tech[(s["section"], row["name"])] = (base_id, vid, tname, row["name"])
                tech_map[f"{base_id}|{vid}"] = {
                    "item_id": base_id,
                    "variant_id": vid,
                    "item_name": s["section"],
                    "variant_name": row["name"],
                    "section": s["section"],
                    "kind": "split",
                    "capacity": cap,
                }
                expected[f"{base_id}|{vid}"] = [c["user"] for c in row["claims"]]
        elif kind == "split_group":
            iid = f"{slug}_g{si}"
            name = s["items"][0]["name"]
            qty = s["items"][0].get("quantity")
            box = qty if qty else max(1, len(s["items"][0]["claims"]))
            items.append(
                {
                    "item_id": iid,
                    "name": name,
                    "kind": "split",
                    "unit_price_cents": s["items"][0].get("unit_price_cents") or 0,
                    "box_size": box,
                    "max_quantity": None,
                    "aliases": [],
                    "variants": [],
                }
            )
            tech[(s["section"], name)] = (iid, None, name, name)
            tech_map[f"{iid}|"] = {
                "item_id": iid,
                "variant_id": "",
                "item_name": name,
                "variant_name": "整套",
                "section": s["section"],
                "kind": "split",
                "capacity": box,
            }
            expected[f"{iid}|"] = [c["user"] for c in s["items"][0]["claims"]]
        elif kind == "single":
            for ri, row in enumerate(s["items"]):
                iid = f"{slug}_d{si}_{ri}"
                items.append(
                    {
                        "item_id": iid,
                        "name": row["name"],
                        "kind": "single",
                        "unit_price_cents": row.get("price_cents") or row.get("original_price_cents") or 0,
                        "box_size": None,
                        "max_quantity": row.get("quantity"),
                        "aliases": [],
                        "variants": [],
                    }
                )
                tech[(s["section"], row["name"])] = (iid, None, row["name"], row["name"])
                tech_map[f"{iid}|"] = {
                    "item_id": iid,
                    "variant_id": "",
                    "item_name": row["name"],
                    "variant_name": "",
                    "section": s["section"],
                    "kind": "single",
                    "capacity": None,
                }
                expected[f"{iid}|"] = [c["user"] for c in row["claims"]]

    round_config = {"round_id": slug, "title": sec["title"], "group_id": group, "items": items}
    json.dump(round_config, open(os.path.join(d, "round_config.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=2)

    base_ts = 1779000000000
    lines = []
    for m in msgs:
        # technical text uses the variant technical name; group/single use real name
        parts = []
        for c in m["claims"]:
            key = (c["section"], c["variant"] or c["item"])
            if key in tech:
                parts.append("排" + tech[key][2])
            else:
                parts.append("排" + (c["variant"] or c["item"]))
        ts = base_ts + m["pos"] * 1000
        lines.append(
            json.dumps(
                {
                    "source_sequence": m["seq"],
                    "group_id": group,
                    "user_id": m["user"],
                    "nickname": m["display"],
                    "message_id": f"{slug}_m{m['seq']}",
                    "timestamp_ms": ts,
                    "text": " ".join(parts),
                    "attachments": [],
                    "reply_to_message_id": None,
                    "is_admin": False,
                },
                ensure_ascii=False,
            )
        )
    open(os.path.join(d, "queue.jsonl"), "w", encoding="utf-8").write("\n".join(lines) + "\n")
    json.dump(expected, open(os.path.join(d, "expected_allocation.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=2)
    json.dump(tech_map, open(os.path.join(d, "tech_map.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=2)
    return d, len(items), len(lines), len(expected)


def run_and_compare(slug, d):
    out = os.path.join(d, "out")
    r = subprocess.run(
        [EXE, "simulate", "--round-config", os.path.join(d, "round_config.json"), "--queue", os.path.join(d, "queue.jsonl"), "--out", out],
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    result = json.load(open(os.path.join(out, "result.json"), encoding="utf-8"))
    expected = json.load(open(os.path.join(d, "expected_allocation.json"), encoding="utf-8"))

    actual = {}
    for ia in result["final_snapshot"]["item_allocations"]:
        key = f"{ia['item_id']}|{ia.get('variant_id') or ''}"
        users = []
        if ia.get("kind") == "single":
            for s in ia.get("singles", []):
                uid = s["user_id"]
                uid = uid["0"] if isinstance(uid, dict) else uid
                users.extend([uid] * s["quantity"])
        else:
            boxes = sorted(ia.get("boxes", []), key=lambda b: b["box_index"])
            for b in boxes:
                for s in sorted(b["slots"], key=lambda x: x["slot_index"]):
                    if str(s["status"]).lower() == "filled" and s.get("user_id"):
                        uid = s["user_id"]
                        uid = uid["0"] if isinstance(uid, dict) else uid
                        users.append(uid)
        actual[key] = users

    passed = failed = 0
    fails = []
    for k, exp in expected.items():
        act = actual.get(k, [])
        if exp == act:
            passed += 1
        else:
            failed += 1
            fails.append((k, exp, act))
    print(f"--- {slug} ---")
    print(r.stdout.strip().splitlines()[-2] if len(r.stdout.strip().splitlines()) >= 2 else r.stdout.strip())
    print(f"cells: pass={passed} fail={failed} / total={len(expected)}")
    for k, exp, act in fails[:30]:
        print(f"  FAIL {k}\n     expected: {exp}\n     actual:   {act}")
    return failed


def main():
    total_fail = 0
    for s in SAMPLES:
        d, ni, nm, ne = build(s["slug"], s["group"])
        total_fail += run_and_compare(s["slug"], d)
    print("ALL PASS" if total_fail == 0 else f"TOTAL FAIL={total_fail}")


if __name__ == "__main__":
    main()
