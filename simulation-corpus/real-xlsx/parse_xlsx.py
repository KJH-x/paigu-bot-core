# -*- coding: utf-8 -*-
"""Parse real paigu result xlsx into canonical JSON.

Sheet1 semantics (authoritative): rows = item/variant, columns to the right = claim
order (append); multiple rows in the same column = concurrent claims.
Proxy notation: A（代B）/ A(代B) -> identity A, display A(代B) with half-width parens.
"""
import json
import os
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

ROOT = r"C:\_CustomPrograms\AnAgent\workspace\paigu-bot-core-20260510"
OUT = os.path.join(ROOT, "simulation-corpus", "real-xlsx")

SAMPLES = [
    {"slug": "月行水上", "file": "月行水上.xlsx", "group": "30001"},
    {"slug": "覆雪于冬", "file": "覆雪于冬.xlsx", "group": "30002"},
]

NS = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
RNS = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"

HEADER_KEYWORDS = {"单价", "调价", "原价", "折后价", "数量", "认购计数", "cn", "CN"}
GENERIC_LABELS = HEADER_KEYWORDS | {"名称", "总数", "总数校验", ""}


def col_to_num(letters):
    n = 0
    for ch in letters:
        n = n * 26 + (ord(ch) - 64)
    return n


def num_to_col(n):
    s = ""
    while n > 0:
        n, r = divmod(n - 1, 26)
        s = chr(65 + r) + s
    return s


def read_xml_grid(path, sheet_target):
    z = zipfile.ZipFile(path)
    shared = [
        "".join(t.text or "" for t in si.iter(NS + "t"))
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall(NS + "si")
    ]
    sh = ET.fromstring(z.read("xl/" + sheet_target.lstrip("/")))
    grid = {}
    for row in sh.iter(NS + "row"):
        for c in row.findall(NS + "c"):
            ref = c.get("r")
            m = re.match(r"([A-Z]+)(\d+)", ref)
            col = col_to_num(m.group(1))
            r = int(m.group(2))
            t = c.get("t")
            v = c.find(NS + "v")
            if v is not None:
                val = shared[int(v.text)] if t == "s" else v.text
            elif t == "inlineStr":
                val = "".join(x.text or "" for x in c.iter(NS + "t"))
            else:
                val = None
            if val not in (None, ""):
                grid[(r, col)] = val
    return grid


def xml_sheets(path):
    z = zipfile.ZipFile(path)
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    rmap = {r.get("Id"): r.get("Target") for r in rels}
    out = []
    for s in wb.find(NS + "sheets"):
        out.append((s.get("name"), rmap[s.get(RNS + "id")]))
    return out


def read_openpyxl_grid(path, sheet_name):
    import openpyxl

    wb = openpyxl.load_workbook(path, data_only=True)
    ws = wb[sheet_name]
    grid = {}
    for row in ws.iter_rows():
        for c in row:
            if c.value not in (None, ""):
                grid[(c.row, c.column)] = c.value
    return grid


def normalize_name(raw):
    if raw is None:
        return "", ""
    s = str(raw).strip()
    s = s.replace("（", "(").replace("）", ")").replace("：", ":")
    m = re.match(r"^(.*?)\(代(.*)\)$", s)
    if m:
        return m.group(1).strip(), s
    return s, s


def as_int(v):
    try:
        return int(float(str(v)))
    except Exception:
        return None


def as_cents(v):
    try:
        return int(round(float(str(v)) * 100))
    except Exception:
        return None


def row_has_keyword(grid, r, maxc=40):
    for c in range(2, maxc):
        v = grid.get((r, c))
        if v is not None and str(v) in HEADER_KEYWORDS:
            return True
    return False


def header_values(grid, r, maxc=40):
    vals = {}
    for c in range(1, maxc):
        v = grid.get((r, c))
        if v is not None:
            vals[c] = str(v)
    return vals


def parse_sheet1(grid):
    max_row = max((r for (r, _) in grid), default=0)
    sections = []
    pending_title = None
    doc_title = None
    r = 1
    while r <= max_row:
        a = grid.get((r, 1))
        a_str = str(a).strip() if a is not None else ""
        if not a_str:
            r += 1
            continue
        if row_has_keyword(grid, r):
            hv = header_values(grid, r)
            if a_str in GENERIC_LABELS and pending_title:
                section_name = pending_title
            else:
                section_name = a_str
            pending_title = None
            # kind
            vals = set(hv.values())
            if "原价" in vals:
                kind = "single"
            elif "单价" in vals and "数量" in vals:
                kind = "split_group"
            elif "调价" in vals or "折后价" in vals:
                kind = "split_variants"
            else:
                kind = "split_variants"
            # cn column
            cn_col = None
            for c, v in hv.items():
                if v.lower() == "cn":
                    cn_col = c
                    break
            # collect item rows
            items = []
            rr = r + 1
            while rr <= max_row:
                aa = grid.get((rr, 1))
                aa_str = str(aa).strip() if aa is not None else ""
                if not aa_str or aa_str in GENERIC_LABELS:
                    break
                if row_has_keyword(grid, rr):
                    break
                items.append(rr)
                rr += 1
            # infer cn col if not labelled
            if cn_col is None:
                for ir in items:
                    for c in range(2, 40):
                        v = grid.get((ir, c))
                        if v is None:
                            continue
                        s = str(v)
                        if re.search(r"[^\d.\-]", s):
                            cn_col = c
                            break
                    if cn_col is not None:
                        break
            if cn_col is None:
                cn_col = 4

            hdr = {}
            for c, v in hv.items():
                if v not in hdr:
                    hdr[v] = c

            section = {
                "section": section_name,
                "kind": kind,
                "items": [],
            }
            def cell(ir, label):
                c = hdr.get(label)
                return grid.get((ir, c)) if c else None

            for ir in items:
                item_name = str(grid.get((ir, 1))).strip()
                claims = []
                pos = 0
                for col in range(cn_col, 40):
                    v = grid.get((ir, col))
                    if v is None or str(v).strip() == "":
                        continue
                    pos += 1
                    user, display = normalize_name(v)
                    claims.append(
                        {
                            "col": col,
                            "pos": pos,
                            "user": user,
                            "display": display,
                            "raw": str(v).strip(),
                        }
                    )
                entry = {
                    "name": item_name,
                    "unit_price_cents": as_cents(cell(ir, "单价")),
                    "original_price_cents": as_cents(cell(ir, "原价")),
                    "adjustment_cents": as_cents(cell(ir, "调价")),
                    "price_cents": as_cents(cell(ir, "折后价")),
                    "quantity": as_int(cell(ir, "数量")),
                    "claim_count": as_int(cell(ir, "认购计数")),
                    "claims": claims,
                }
                section["items"].append(entry)
            sections.append(section)
            r = rr
            continue
        # non-keyword A row: could be doc title or section title for next header
        nxt = grid.get((r + 1, 1))
        if r == 1 and nxt is not None:
            doc_title = a_str
            r += 1
            continue
        pending_title = a_str
        r += 1
    return doc_title, sections


def parse_sheet3(grid):
    max_row = max((r for (r, _) in grid), default=0)
    max_col = max((c for (_, c) in grid), default=0)
    if max_row < 2:
        return []
    # header row 1: item-group columns = header not in {总数,总价,已交,蓝退红补,cn}
    item_cols = []
    for c in range(2, max_col + 1):
        v = grid.get((1, c))
        if v is None:
            continue
        s = str(v).strip()
        if s in ("总数", "总价", "已交", "蓝退红补", "cn", "CN"):
            continue
        if s.endswith("总数") or s.endswith("总价"):
            continue
        item_cols.append((c, s))
    out = []
    for r in range(2, max_row + 1):
        a = grid.get((r, 1))
        if a is None or str(a).strip() == "":
            continue
        raw = str(a).strip()
        if raw in ("Generated by GGTabulator beta version",):
            continue
        if raw.startswith("E-mail"):
            continue
        identity, display = normalize_name(raw)
        items = []
        for c, _label in item_cols:
            v = grid.get((r, c))
            if v is None:
                continue
            items.extend(parse_item_string(str(v)))
        if not items:
            continue
        out.append({"display": display, "identity": identity, "raw": raw, "items": items})
    return out


ITEM_RE = re.compile(r"([^\d]+?)(\d+)")


def parse_item_string(s):
    s = s.strip()
    if not s:
        return []
    res = []
    for name, qty in ITEM_RE.findall(s):
        name = name.strip()
        if not name:
            continue
        res.append({"name": name, "qty": int(qty)})
    if not res:
        return [{"name": s, "qty": None}]
    joined = "".join(n + q for n, q in ITEM_RE.findall(s))
    if joined != s:
        res.append({"name": s, "qty": None, "unparsed": True})
    return res


def build_messages(sections):
    groups = {}
    order = []
    for si, sec in enumerate(sections):
        for entry in sec["items"]:
            for cl in entry["claims"]:
                key = (cl["pos"], cl["user"])
                if key not in groups:
                    groups[key] = {
                        "pos": cl["pos"],
                        "user": cl["user"],
                        "display": cl["display"],
                        "section_order": si,
                        "claims": [],
                    }
                    order.append(key)
                g = groups[key]
                g["section_order"] = min(g["section_order"], si)
                if cl["display"] not in g["display"].split(" / "):
                    pass
                g["claims"].append(
                    {
                        "section": sec["section"],
                        "item": entry["name"],
                        "variant": entry["name"] if sec["kind"] == "split_variants" else None,
                        "qty": 1,
                    }
                )
    msgs = sorted(order, key=lambda k: (groups[k]["pos"], groups[k]["section_order"], groups[k]["user"]))
    out = []
    for i, k in enumerate(msgs, 1):
        g = groups[k]
        parts = []
        for c in g["claims"]:
            parts.append("排" + (c["variant"] or c["item"]))
        out.append(
            {
                "seq": i,
                "pos": g["pos"],
                "user": g["user"],
                "display": g["display"],
                "text": " ".join(parts),
                "claims": g["claims"],
            }
        )
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    summary = []
    for s in SAMPLES:
        path = os.path.join(ROOT, s["file"])
        slug = s["slug"]
        if s["file"] == "月行水上.xlsx":
            grid = read_openpyxl_grid(path, "Sheet1")
            grid3 = read_openpyxl_grid(path, "Sheet3")
        else:
            keys = [tgt for _name, tgt in xml_sheets(path)]
            grid = read_xml_grid(path, keys[0])
            grid3 = read_xml_grid(path, keys[2]) if len(keys) >= 3 else {}
        doc_title, sections = parse_sheet1(grid)
        who = parse_sheet3(grid3) if grid3 else []
        messages = build_messages(sections)

        d = os.path.join(OUT, slug)
        os.makedirs(d, exist_ok=True)
        json.dump(
            {"sample_id": slug, "title": doc_title or slug, "sections": sections},
            open(os.path.join(d, "sections.json"), "w", encoding="utf-8"),
            ensure_ascii=False,
            indent=2,
        )
        json.dump(who, open(os.path.join(d, "who_whats.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=2)
        json.dump(messages, open(os.path.join(d, "messages.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=2)

        n_items = sum(len(sec["items"]) for sec in sections)
        n_claims = sum(len(e["claims"]) for sec in sections for e in sec["items"])
        summary.append(
            f"{slug}: title={doc_title} sections={len(sections)} items={n_items} claims={n_claims} "
            f"messages={len(messages)} who_whats={len(who)}"
        )
    print("\n".join(summary))


if __name__ == "__main__":
    main()
