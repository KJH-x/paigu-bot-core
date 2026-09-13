// Convert real "event_replay" samples into runnable paigu-bot-core fixtures.
// Usage: node convert.mjs <projectRoot> <outRoot>
import fs from "node:fs";
import path from "node:path";

const projectRoot = process.argv[2];
const outRoot = process.argv[3];

const SAMPLES = [
  { id: "sample1", file: "sample 1.json", group: "20001", fallbackTitle: "未命名" },
  { id: "sample2", file: "sample2.json", group: "20002", fallbackTitle: "未命名" },
];

const cents = (v) => (v === null || v === undefined ? null : Math.round(v * 100));

function uniqueName(section, display, id, items) {
  let name = section && section !== display ? `${section}·${display}` : display;
  const collides = (n) =>
    items.some((it) => it.name.includes(n) || n.includes(it.name));
  if (collides(name)) name = `${display}（${id}）`;
  if (collides(name)) name = `${name}#${id}`;
  return name;
}

function buildSample(sample, meta) {
  const items = [];
  const messages = [];
  const sections = [];
  const allocation = {};

  const uidMap = new Map();
  let uidCounter = 0;
  const uid = (cn) => {
    if (!uidMap.has(cn)) uidMap.set(cn, "u" + ++uidCounter);
    return uidMap.get(cn);
  };

  let ts = 1779000000000;
  let seqNo = 0;
  const tick = () => (ts += 100);

  const pushMsg = (text, cn) => {
    seqNo += 1;
    messages.push({
      source_sequence: seqNo,
      group_id: meta.group,
      user_id: uid(cn),
      nickname: cn,
      message_id: `${meta.id}_m${seqNo}`,
      timestamp_ms: tick(),
      text,
      attachments: [],
      reply_to_message_id: null,
      is_admin: false,
    });
  };

  for (const block of sample.event_replay || []) {
    const section = block.section || "";
    const baseItem = block.item || section;
    const type = block.type || "split_claim";
    const sectionOut = {
      seq: block.seq,
      section,
      item: baseItem,
      type,
      unit_price_cents: cents(block.unit_price),
      total_quantity: block.total_quantity ?? null,
      events: [],
      unclaimed_variants: [],
      ignored: [],
    };

    if (type === "claim") {
      const id = `s${block.seq}`;
      const name = uniqueName(section, baseItem, id, items);
      items.push({
        item_id: id,
        name,
        kind: "split",
        unit_price_cents: cents(block.unit_price) ?? 0,
        box_size: block.total_quantity ?? 1,
        max_quantity: null,
        aliases: [],
        metadata: { section, type, base_item: baseItem },
      });
      for (const ev of block.events || []) {
        pushMsg(`排${name}`, ev.cn);
        sectionOut.events.push({ order: ev.order, cn: ev.cn, variant: null });
      }
      allocation[id] = { kind: "split", slots: (block.events || []).map((e) => e.cn) };
    } else if (type === "split_claim") {
      const variantOrder = [];
      const seen = new Set();
      const addVariant = (v) => {
        if (v != null && !seen.has(v)) {
          seen.add(v);
          variantOrder.push(v);
        }
      };
      for (const ev of block.events || []) {
        if (ev.type === "package_tail") (ev.variants || []).forEach(addVariant);
        else addVariant(ev.variant);
      }
      for (const uv of block.unclaimed_variants || []) {
        addVariant(typeof uv === "string" ? uv : uv.variant);
      }

      const nameOf = new Map();
      variantOrder.forEach((v, i) => {
        const id = `s${block.seq}v${i + 1}`;
        const name = uniqueName(section, v, id, items);
        nameOf.set(v, name);

        const ev = (block.events || []).find(
          (e) => e.variant === v && e.type !== "package_tail"
        );
        let priceCents = null;
        if (ev) {
          priceCents =
            cents(ev.adjusted_price) ??
            cents(ev.price) ??
            (block.unit_price != null
              ? cents(block.unit_price) + cents(ev.adjustment || 0)
              : cents(ev.adjustment));
        }
        if (priceCents === null) {
          const uv = (block.unclaimed_variants || []).find(
            (u) => (typeof u === "string" ? u : u.variant) === v
          );
          if (uv && typeof uv === "object") {
            priceCents =
              block.unit_price != null
                ? cents(block.unit_price) + cents(uv.adjustment || 0)
                : cents(uv.adjustment);
          }
        }

        const claimCount = (block.events || []).filter(
          (e) => e.type !== "package_tail" && e.variant === v
        ).length;

        items.push({
          item_id: id,
          name,
          kind: "split",
          unit_price_cents: priceCents ?? 0,
          box_size: Math.max(1, claimCount),
          max_quantity: null,
          aliases: [],
          metadata: {
            section,
            type,
            base_item: baseItem,
            variant: v,
            claim_count: claimCount,
            unclaimed: claimCount === 0,
          },
        });
      });

      for (const ev of block.events || []) {
        if (ev.type === "package_tail") {
          for (const v of ev.variants || []) {
            pushMsg(`包尾${nameOf.get(v)}`, ev.cn);
            sectionOut.events.push({
              order: ev.order,
              cn: ev.cn,
              variant: v,
              package_tail: true,
            });
          }
        } else {
          pushMsg(`排${nameOf.get(ev.variant)}`, ev.cn);
          sectionOut.events.push({
            order: ev.order,
            cn: ev.cn,
            variant: ev.variant,
          });
        }
      }
      for (const uv of block.unclaimed_variants || []) {
        sectionOut.unclaimed_variants.push(
          typeof uv === "string" ? uv : uv.variant
        );
      }

      for (const v of variantOrder) {
        const id = variantOrder.indexOf(v) + 1;
        const itemId = `s${block.seq}v${id}`;
        const slots = [];
        for (const ev of block.events || []) {
          if (ev.type === "package_tail") {
            if ((ev.variants || []).includes(v)) slots.push(ev.cn);
          } else if (ev.variant === v) {
            slots.push(ev.cn);
          }
        }
        allocation[itemId] = { kind: "split", slots };
      }
    } else if (type === "direct_claim") {
      const info = new Map();
      for (const ev of block.events || []) {
        if (!info.has(ev.item)) {
          const id = `d${block.seq}_${info.size + 1}`;
          const name = uniqueName(section, ev.item, id, items);
          const priceCents = cents(ev.price) ?? cents(ev.original_price) ?? 0;
          info.set(ev.item, { id, name });
          items.push({
            item_id: id,
            name,
            kind: "single",
            unit_price_cents: priceCents,
            box_size: null,
            max_quantity: null,
            aliases: [],
            metadata: { section, type },
          });
        }
      }
      for (const ev of block.events || []) {
        const { name } = info.get(ev.item);
        pushMsg(`排${name}`, ev.cn);
        sectionOut.events.push({ order: ev.order, cn: ev.cn, item: ev.item });
      }
      for (const [itemName, { id }] of info) {
        const count = (block.events || []).filter((e) => e.item === itemName).length;
        allocation[id] = { kind: "single", count };
      }
    }

    sections.push(sectionOut);
  }

  for (const ig of sample.ignored || []) {
    sections.push({
      seq: null,
      section: "(ignored)",
      item: ig.item,
      type: "ignored",
      unit_price_cents: null,
      total_quantity: null,
      events: [],
      unclaimed_variants: [],
      ignored: [ig],
    });
  }

  const expected = {
    sample_id: meta.id,
    title: sample.title || meta.fallbackTitle,
    source_file: meta.file,
    group_id: meta.group,
    round_id: meta.id,
    sections,
  };

  const roundConfig = {
    round_id: meta.id,
    title: expected.title,
    group_id: meta.group,
    items,
  };

  return { roundConfig, messages, expected, allocation };
}

function writeSample(meta) {
  const src = path.join(projectRoot, meta.file);
  const raw = JSON.parse(fs.readFileSync(src, "utf8"));
  const { roundConfig, messages, expected, allocation } = buildSample(raw, meta);

  const dir = path.join(outRoot, meta.id);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(
    path.join(dir, "round_config.json"),
    JSON.stringify(roundConfig, null, 2)
  );
  fs.writeFileSync(
    path.join(dir, "queue.jsonl"),
    messages.map((m) => JSON.stringify(m)).join("\n") + "\n"
  );
  fs.writeFileSync(
    path.join(dir, "expected.json"),
    JSON.stringify(expected, null, 2)
  );
  fs.writeFileSync(
    path.join(dir, "expected_allocation.json"),
    JSON.stringify({ sample_id: meta.id, items: allocation }, null, 2)
  );

  return {
    id: meta.id,
    items: roundConfig.items.length,
    messages: messages.length,
    sections: expected.sections.length,
  };
}

fs.mkdirSync(outRoot, { recursive: true });
const summary = SAMPLES.map(writeSample);
console.log(JSON.stringify(summary, null, 2));
