#!/usr/bin/env node
// §0/§1 术语迁移：把 JSON 文件里的旧键就地改写为新键（仅键名，不动值）。
//   变体→子项：variants→subitems, variant_id→subitem_id, variant_name→subitem_name,
//             resolved_variant_id→resolved_subitem_id
// 目标：config/app.json、data/rounds/*.json、data/settlement/*.json、web/replay_view.json
// 用法：node scripts/migrate-terminology.mjs [--dry-run] [--dir <extra-dir> ...]
import { readFileSync, writeFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';

const RENAMES = [
  ['variants', 'subitems'],
  ['variant_id', 'subitem_id'],
  ['variant_name', 'subitem_name'],
  ['resolved_variant_id', 'resolved_subitem_id'],
];

const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const extraDirs = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--dir' && args[i + 1]) extraDirs.push(args[++i]);
}

function collect(dir, out) {
  if (!existsSync(dir)) return;
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    let st;
    try { st = statSync(p); } catch { continue; }
    if (st.isDirectory()) collect(p, out);
    else if (extname(p).toLowerCase() === '.json') out.push(p);
  }
}

function renameKeys(value) {
  if (Array.isArray(value)) {
    let changed = false;
    for (let i = 0; i < value.length; i++) {
      const [cv, cc] = renameKeys(value[i]);
      value[i] = cv;
      changed = changed || cc;
    }
    return [value, changed];
  }
  if (value && typeof value === 'object') {
    let changed = false;
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      let nk = k;
      for (const [from, to] of RENAMES) {
        if (k === from) { nk = to; break; }
      }
      if (nk !== k) changed = true;
      const [cv, cc] = renameKeys(v);
      out[nk] = cv;
      changed = changed || cc;
    }
    return [out, changed];
  }
  return [value, false];
}

const files = [];
collect('config', files);
collect('data/rounds', files);
collect('data/settlement', files);
for (const d of extraDirs) collect(d, files);
if (existsSync('web/replay_view.json')) files.push('web/replay_view.json');

let migrated = 0;
for (const f of files) {
  let raw;
  try { raw = readFileSync(f, 'utf8'); } catch { continue; }
  let parsed;
  try { parsed = JSON.parse(raw); } catch { continue; }
  const [next, changed] = renameKeys(parsed);
  if (!changed) continue;
  migrated++;
  if (dryRun) { console.log(`[dry-run] would migrate ${f}`); continue; }
  writeFileSync(f, JSON.stringify(next, null, 2) + '\n', 'utf8');
  console.log(`migrated ${f}`);
}
console.log(`done: ${migrated} file(s) ${dryRun ? '(dry-run)' : 'migrated'}, scanned ${files.length}`);
