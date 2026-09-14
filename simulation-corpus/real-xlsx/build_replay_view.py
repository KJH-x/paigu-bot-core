# -*- coding: utf-8 -*-
"""Assemble viewer/replay_view.json from engine output + xlsx-derived canonical data."""
import json
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = os.path.join(ROOT, "simulation-corpus", "real-xlsx")
VIEWER = os.path.join(ROOT, "viewer")
SAMPLES = ["月行水上", "覆雪于冬"]


def load(p):
    return json.load(open(p, encoding="utf-8"))


def board_from_snapshot(snap):
    board = {}
    for ia in snap["item_allocations"]:
        key = f"{ia['item_id']}|{ia.get('variant_id') or ''}"
        slots = []
        if ia.get("kind") == "single":
            idx = 0
            for s in ia.get("singles", []):
                uid = s["user_id"]
                uid = uid["0"] if isinstance(uid, dict) else uid
                for _ in range(s["quantity"]):
                    idx += 1
                    slots.append({"slot": idx, "user": uid, "status": "filled"})
        else:
            for b in sorted(ia.get("boxes", []), key=lambda b: b["box_index"]):
                for s in sorted(b["slots"], key=lambda x: x["slot_index"]):
                    st = str(s["status"]).lower()
                    uid = s.get("user_id")
                    if uid:
                        uid = uid["0"] if isinstance(uid, dict) else uid
                    slots.append(
                        {
                            "slot": s["slot_index"],
                            "user": uid,
                            "status": {"filled": "filled", "empty": "empty", "lockedempty": "locked", "adminreserved": "admin"}.get(st, st),
                        }
                    )
        board[key] = slots
    return board


def diff_boards(prev, cur):
    changed = []
    for key, slots in cur.items():
        ps = {s["slot"]: s for s in prev.get(key, [])}
        for s in slots:
            p = ps.get(s["slot"])
            before = p["user"] if p else None
            after = s["user"]
            if before != after:
                reason = "NewClaimFilled" if not before else ("CancelReleased" if not after else "AutoMovedForward")
                item_id, vid = key.split("|", 1)
                changed.append({"item_id": item_id, "variant_id": vid, "slot": s["slot"], "before": before, "after": after, "reason": reason})
    for key, slots in prev.items():
        cs = {s["slot"] for s in cur.get(key, [])}
        for s in slots:
            if s["slot"] not in cs and s["user"]:
                item_id, vid = key.split("|", 1)
                changed.append({"item_id": item_id, "variant_id": vid, "slot": s["slot"], "before": s["user"], "after": None, "reason": "CancelReleased"})
    return changed


def build(slug):
    d = os.path.join(OUT, slug)
    sec = load(os.path.join(d, "sections.json"))
    msgs = load(os.path.join(d, "messages.json"))
    tech_map = load(os.path.join(d, "tech_map.json"))
    expected = load(os.path.join(d, "expected_allocation.json"))
    who = load(os.path.join(d, "who_whats.json"))
    result = load(os.path.join(d, "out", "result.json"))

    # items grouped by item_id, preserving tech_map order
    items = []
    index = {}
    for key, meta in tech_map.items():
        iid = meta["item_id"]
        if iid not in index:
            index[iid] = {
                "item_id": iid,
                "name": meta["item_name"],
                "kind": meta["kind"],
                "section": meta["section"],
                "variants": [],
            }
            items.append(index[iid])
        if meta["kind"] == "split":
            index[iid]["variants"].append(
                {"variant_id": meta["variant_id"], "name": meta["variant_name"], "capacity": meta.get("capacity")}
            )

    # messages: zip canonical messages with engine outcomes
    outcomes = result["outcomes"]
    out_msgs = []
    for i, m in enumerate(msgs):
        o = outcomes[i] if i < len(outcomes) else {}
        claims = []
        disp_parts = []
        for c in m["claims"]:
            row = c["variant"] or c["item"]
            t = tech_map.get(f"__none__")  # placeholder
            # find tech entry by section+row
            found = None
            for key, meta in tech_map.items():
                if meta["section"] == c["section"] and meta["variant_name"] == row:
                    found = (key, meta)
                    break
            if found:
                key, meta = found
                claims.append(
                    {
                        "item_id": meta["item_id"],
                        "item_name": meta["item_name"],
                        "variant_id": meta["variant_id"],
                        "variant_name": meta["variant_name"],
                        "qty": c["qty"],
                    }
                )
                disp_parts.append("排" + (meta["variant_name"] or meta["item_name"]))
            else:
                disp_parts.append("排" + row)
        out_msgs.append(
            {
                "seq": m["seq"],
                "pos": m["pos"],
                "user": m["user"],
                "display": m["display"],
                "text": " ".join(disp_parts),
                "status": o.get("status", ""),
                "detail": o.get("detail", ""),
                "claims": claims,
            }
        )

    # steps
    steps_out = []
    prev = {}
    for st in result["replay"]["steps"]:
        board = board_from_snapshot(st["allocation_snapshot"])
        changed = diff_boards(prev, board)
        rid = st.get("raw_message_id") or ""
        m = re.search(r"_m(\d+)$", rid)
        mseq = int(m.group(1)) if m else None
        steps_out.append({"index": st["step_index"], "message_seq": mseq, "board": board, "changed": changed})
        prev = board

    return {
        "sample_id": slug,
        "title": sec["title"],
        "items": items,
        "messages": out_msgs,
        "steps": steps_out,
        "who_whats": who,
        "expected": expected,
    }


def main():
    os.makedirs(os.path.join(VIEWER, "data"), exist_ok=True)
    for slug in SAMPLES:
        rv = build(slug)
        json.dump(rv, open(os.path.join(VIEWER, "data", f"{slug}.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
        if slug == SAMPLES[0]:
            json.dump(rv, open(os.path.join(VIEWER, "replay_view.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
        print(f"{slug}: items={len(rv['items'])} messages={len(rv['messages'])} steps={len(rv['steps'])} who={len(rv['who_whats'])} expected={len(rv['expected'])}")


if __name__ == "__main__":
    main()
