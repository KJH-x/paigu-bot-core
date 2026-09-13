// Compare simulate output (out/result.json) against the sample's original event order.
// Usage: node verify-samples.mjs <samplesRoot>
import fs from "node:fs";
import path from "node:path";

const root = process.argv[2];

function readJsonl(p) {
  return fs
    .readFileSync(p, "utf8")
    .split(/\r?\n/)
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l));
}

function checkSample(id) {
  const dir = path.join(root, id);
  const queue = readJsonl(path.join(dir, "queue.jsonl"));
  const nickToUid = new Map(queue.map((m) => [m.nickname, m.user_id]));
  const expected = JSON.parse(
    fs.readFileSync(path.join(dir, "expected_allocation.json"), "utf8")
  ).items;
  const result = JSON.parse(
    fs.readFileSync(path.join(dir, "out", "result.json"), "utf8")
  );

  const actual = new Map();
  for (const ia of result.final_snapshot.item_allocations) {
    const boxes = [...ia.boxes].sort((a, b) => a.box_index - b.box_index);
    const uids = [];
    for (const b of boxes) {
      for (const s of [...b.slots].sort((x, y) => x.slot_index - y.slot_index)) {
        const st = String(s.status).toLowerCase();
        const uid =
          s.user_id && typeof s.user_id === "object" ? s.user_id["0"] : s.user_id;
        if (st === "filled" && uid) uids.push(uid);
      }
    }
    const singleQty = (ia.singles || []).reduce((a, s) => a + s.quantity, 0);
    actual.set(ia.item_id, { uids, singleQty });
  }

  let pass = 0;
  let fail = 0;
  const failures = [];
  for (const [itemId, exp] of Object.entries(expected)) {
    const act = actual.get(itemId) || { uids: [], singleQty: 0 };
    if (exp.kind === "split") {
      const want = exp.slots.map((cn) => nickToUid.get(cn));
      const ok =
        want.length === act.uids.length &&
        want.every((u, i) => u === act.uids[i]);
      if (ok) pass++;
      else {
        fail++;
        failures.push(
          `${itemId}: expected [${want.join(",")}] got [${act.uids.join(",")}]`
        );
      }
    } else {
      if (exp.count === act.singleQty) pass++;
      else {
        fail++;
        failures.push(
          `${itemId}: expected single count ${exp.count} got ${act.singleQty}`
        );
      }
    }
  }

  return { id, items: Object.keys(expected).length, pass, fail, failures };
}

const summary = ["sample1", "sample2"].map(checkSample);
for (const s of summary) {
  console.log(
    `${s.id}: items=${s.items} pass=${s.pass} fail=${s.fail}`
  );
  for (const f of s.failures.slice(0, 20)) console.log("  FAIL " + f);
}
const totalFail = summary.reduce((a, s) => a + s.fail, 0);
console.log(totalFail === 0 ? "ALL PASS" : `TOTAL FAIL=${totalFail}`);
process.exit(totalFail === 0 ? 0 : 1);
