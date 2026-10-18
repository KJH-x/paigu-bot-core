"""Read private source workbooks without changing the read-only corpus.

All generated fixtures and outputs stay in an OS temporary directory. The old
fixture builder uses variant keys; only its generated schema keys are adapted
to the canonical subitem keys, never the expected allocation or ordering.
No member names, message text or configuration values are printed.
"""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "simulation-corpus"


def fingerprint():
    files = list(CORPUS.rglob("*")) + list(ROOT.glob("*.xlsx"))
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in files if p.is_file()}


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, CORPUS / "real-xlsx" / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    before = fingerprint()
    parser = module("private_parser", "parse_xlsx.py")
    builder = module("private_builder", "build_fixtures.py")
    executable = ROOT / "target" / "debug" / ("paigu-bot-core.exe" if os.name == "nt" else "paigu-bot-core")
    messages = groups = 0
    try:
        with tempfile.TemporaryDirectory(prefix="paigu-readonly-xlsx-") as temp:
            parser.OUT = builder.OUT = temp
            # Parser diagnostics contain private workbook titles and are not
            # useful for the aggregate pass/fail report.
            with contextlib.redirect_stdout(io.StringIO()):
                parser.main()
            for sample in builder.SAMPLES:
                directory, _, count, _ = builder.build(sample["slug"], sample["group"])
                directory = Path(directory)
                config_path = directory / "round_config.json"
                config = json.loads(config_path.read_text(encoding="utf-8"))
                for item in config["items"]:
                    item["subitems"] = item.pop("variants")
                    for subitem in item["subitems"]:
                        subitem["subitem_id"] = subitem.pop("variant_id")
                config_path.write_text(json.dumps(config, ensure_ascii=False), encoding="utf-8")
                run = subprocess.run([str(executable), "simulate", "--round-config", str(config_path),
                                      "--queue", str(directory / "queue.jsonl"), "--out", str(directory / "out")],
                                     capture_output=True, encoding="utf-8")
                if run.returncode:
                    raise RuntimeError("isolated workbook simulation failed; private output withheld")
                result = json.loads((directory / "out" / "result.json").read_text(encoding="utf-8"))
                expected = json.loads((directory / "expected_allocation.json").read_text(encoding="utf-8"))
                actual = {}
                for allocation in result["final_snapshot"]["item_allocations"]:
                    key = allocation["item_id"] + "|" + (allocation.get("subitem_id") or "")
                    users = []
                    def uid(value):
                        return value.get("0") if isinstance(value, dict) else value
                    if allocation.get("kind") == "single":
                        for single in allocation.get("singles", []):
                            users.extend([uid(single["user_id"])] * single["quantity"])
                    else:
                        for box in sorted(allocation.get("boxes", []), key=lambda b: b["box_index"]):
                            for slot in sorted(box["slots"], key=lambda s: s["slot_index"]):
                                if str(slot["status"]).lower() == "filled" and slot.get("user_id"):
                                    users.append(uid(slot["user_id"]))
                    actual[key] = users
                failed = sum(actual.get(key, []) != value for key, value in expected.items())
                if failed:
                    raise RuntimeError(f"workbook allocation mismatch: {failed}/{len(expected)} groups; private values withheld")
                messages += count
                groups += len(expected)
    finally:
        if fingerprint() != before:
            raise RuntimeError("read-only corpus changed")
    print(f"ALL PASS: 2 workbooks, {messages} messages, {groups} allocation groups; source hashes unchanged")


if __name__ == "__main__":
    main()
