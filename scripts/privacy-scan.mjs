#!/usr/bin/env node
// 隐私/密钥扫描：只扫描 git 跟踪文件（git ls-files），命中即报错（退出码非 0）。
// 本脚本不得包含任何真实昵称/群号/内网 IP 字面量；仅使用通用模式。
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// 以脚本位置定位仓库根（不依赖 CWD）
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const SELF = "scripts/privacy-scan.mjs";
const PLACEHOLDER_NICK = /^成员\d{2}$/;
// A generic keyboard/formatting term is not an identifying name by itself.
const NON_IDENTIFYING = new Set(["空格"]);
const ALLOWED_DATA = new Set(["data/members.example.json"]);
const stagedText = new Map();
const scannedText = new Map();
function isPublicPlaceholder(value) {
  return PLACEHOLDER_NICK.test(value) || /^user_[a-z]$/.test(value) || value === "123456789";
}

// 只对「有辨识度」的昵称做全局搜索，避免 2–4 位通用英文短词（如 IDE/web/data）
// 造成大量误报；中文昵称、带数字/符号或较长的昵称仍会被检出。
function isDistinctiveNickname(n) {
  if (n.length < 2) return false;
  const nonAscii = /[^\x00-\x7F]/.test(n);
  if (nonAscii) return true;
  if (n.length >= 5) return true;
  if (/[0-9]/.test(n) && /[A-Za-z]/.test(n)) return true;
  return false;
}

const SECRET_PATTERNS = [
  { name: "openai-like key (sk-)", re: /sk-[A-Za-z0-9]{16,}/ },
  { name: "aws access key (AKIA)", re: /AKIA[0-9A-Z]{16}/ },
  { name: "github token (ghp_)", re: /ghp_[A-Za-z0-9]{20,}/ },
  { name: "slack token (xox)", re: /xox[abprs]-[A-Za-z0-9-]{10,}/ },
  { name: "private key block", re: /-----BEGIN [A-Z ]*PRIVATE KEY-----/ },
  { name: "bearer token", re: /Bearer\s+[A-Za-z0-9._-]{12,}/ },
];

const IP_PATTERNS = [
  { name: "private 10/8", re: /\b10\.\d{1,3}\.\d{1,3}\.\d{1,3}\b/ },
  { name: "private 172.16/12", re: /\b172\.(1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}\b/ },
  { name: "private 192.168/16", re: /\b192\.168\.\d{1,3}\.\d{1,3}\b/ },
  { name: "link-local 169.254/16", re: /\b169\.254\.\d{1,3}\.\d{1,3}\b/ },
];

function gitLsFiles() {
  const out = execFileSync("git", ["ls-files", "--stage", "-z"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
  });
  const entries = out.split("\0").filter(Boolean).map(entry => {
    const split = entry.indexOf("\t"), metadata = entry.slice(0, split).split(" ");
    if (split < 0 || !/^[a-f0-9]+$/.test(metadata[1])) throw new Error("invalid index entry");
    return {file: entry.slice(split + 1), object: metadata[1]};
  });
  const objects = [...new Set(entries.map(e => e.object))], blobs = new Map();
  if (objects.length) {
    const batch = execFileSync("git", ["cat-file", "--batch"], {cwd: REPO_ROOT, input: objects.join("\n") + "\n", maxBuffer: 64 * 1024 * 1024});
    let offset = 0;
    for (const object of objects) {
      const end = batch.indexOf(10, offset), header = batch.subarray(offset, end).toString("utf8").split(" ");
      const size = Number(header[2]);
      if (end < 0 || header[0] !== object || !Number.isSafeInteger(size) || size < 0 || end + size + 2 > batch.length) throw new Error("incomplete index object");
      const raw = batch.subarray(end + 1, end + 1 + size);
      blobs.set(object, header[1] === "blob" && !raw.includes(0) ? raw.toString("utf8") : "");
      offset = end + size + 2;
    }
  }
  for (const entry of entries) stagedText.set(entry.file, (stagedText.get(entry.file) || "") + "\n" + blobs.get(entry.object));
  return [...new Set(entries.map(e => e.file))];
}

function readText(rel) {
  if (scannedText.has(rel)) return scannedText.get(rel);
  const abs = path.join(REPO_ROOT, rel);
  let working = "";
  try {
    const raw = readFileSync(abs);
    if (!raw.includes(0)) working = raw.toString("utf8");
  } catch {
    // Missing/deleted working files still have an index blob to inspect.
  }
  const text = (stagedText.get(rel) || "") + "\n" + working;
  scannedText.set(rel, text);
  return text;
}

function readPrivateJson(rel, errors, label = rel) {
  const abs = path.resolve(REPO_ROOT, rel);
  if (!existsSync(abs)) return [];
  try {
    return JSON.parse(readFileSync(abs, "utf8"));
  } catch {
    errors.push({file: label, kind: "private-source-unreadable", detail: "本地隐私源无法读取或解析，未跳过保护；请在本机核对"});
    return null;
  }
}

function addName(names, value) {
  if (typeof value !== "string") return;
  const raw = value.trim(), normalized = raw.normalize("NFKC");
  const clean = normalized.split(/[（(\s]/u)[0].replace(/[、。，．.·・:：;；!！?？~～]+$/u, "");
  for (const n of [raw, normalized, clean]) {
    if (!PLACEHOLDER_NICK.test(n) && !NON_IDENTIFYING.has(n) && isDistinctiveNickname(n)) names.add(n);
  }
}

function collectNicknames(data, names, identifiers) {
  if (!data) return;
  const list = Array.isArray(data) ? data : Array.isArray(data.members) ? data.members : [];
  for (const m of list) {
    if (!m || typeof m !== "object") continue;
    for (const field of ["nickname", "card", "cn", "resolved", "display_name"]) addName(names, m[field]);
    for (const alias of Array.isArray(m.aliases) ? m.aliases : []) addName(names, alias);
    const id = String(m.user_id ?? "");
    if (/^\d{6,}$/.test(id)) identifiers.add(id);
  }
}

function flattenStrings(value, out) {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => flattenStrings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => flattenStrings(v, out));
}

function collectConfigLeaks(local, example, identifiers) {
  const exampleArr = [];
  flattenStrings(example, exampleArr);
  const exampleStrings = new Set(exampleArr.filter(isPublicPlaceholder));

  const localStrings = [];
  flattenStrings(local, localStrings);

  const distinctive = new Set([...identifiers].filter(v => !exampleStrings.has(v)));
  for (const v of localStrings) {
    if (exampleStrings.has(v)) continue;
    const t = v.trim();
    if (/^\d{6,}$/.test(t)) distinctive.add(t); // 群号 / user_id
    else if (/^[A-Za-z0-9_./-]{20,}$/.test(t) && /[0-9]/.test(t)) distinctive.add(t); // token-like
  }
  return [...distinctive];
}

function main() {
  // Vendor dictionaries only skip name matching, never secret/IP/path checks.
  const VENDOR_PREFIX = "web/vendor/";
  const tracked = gitLsFiles();
  const findings = [];
  const add = (file, kind, detail) => findings.push({ file, kind, detail });
  const local = readPrivateJson("config/app.json", findings) || {};
  const example = readPrivateJson("config.example.json", findings) || {};
  const names = new Set(), identifiers = new Set(), configs = [local];
  const nickSources = new Map([["data/members.json", "data/members.json"], ["data/members.seed.json", "data/members.seed.json"]]);
  if (typeof local.members?.cache_path === "string" && local.members.cache_path.trim()) nickSources.set(local.members.cache_path, "configured-member-cache");
  for (const [source, label] of nickSources) collectNicknames(readPrivateJson(source, findings, label), names, identifiers);
  const roundDirectory = path.join(REPO_ROOT, "data", "rounds");
  if (existsSync(roundDirectory)) {
    try {
      for (const file of readdirSync(roundDirectory, {withFileTypes: true})) {
        if (file.isFile() && file.name.endsWith(".json")) configs.push(readPrivateJson(path.join(roundDirectory, file.name), findings, "local-round-library") || {});
      }
    } catch {
      add("local-round-library", "private-source-unreadable", "本地轮次库无法读取，未跳过保护；请在本机核对");
    }
  }
  for (const cfg of configs) {
    collectNicknames(cfg.members?.cn_overrides || [], names, identifiers);
    for (const name of cfg.round?.priority_users || cfg.priority_users || []) addName(names, name);
  }
  const publicValues = [];
  flattenStrings(example, publicValues);
  // Only known placeholders are exempt; editing the example cannot bless a
  // private identity or token copied into the same staged change.
  for (const value of publicValues.filter(isPublicPlaceholder)) names.delete(value);

  // 1) 禁止入库路径
  for (const f of tracked) {
    if (f.endsWith(".xlsx") || f.endsWith(".xls")) add(f, "tracked-spreadsheet", "*.xlsx/*.xls 不得入库");
    if (f.startsWith("config/")) add(f, "tracked-config", "config/** 不得入库");
    if (f.startsWith("data/") && !ALLOWED_DATA.has(f)) add(f, "tracked-data", "data/**（除 members.example.json）不得入库");
  }

  // 2) 密钥形态
  for (const f of tracked) {
    const text = readText(f);
    if (text == null) continue;
    for (const p of SECRET_PATTERNS) {
      if (p.re.test(text)) add(f, "secret", p.name);
    }
  }

  // 3) 内网 IP
  for (const f of tracked) {
    const text = readText(f);
    if (text == null) continue;
    for (const p of IP_PATTERNS) {
      if (p.re.test(text)) add(f, "private-ip", p.name);
    }
  }

  // 4) 群名片/CN/别名与清洗后称呼；输出不包含匹配内容或私有路径。
  for (const f of tracked) {
    if (f === SELF || f.startsWith(VENDOR_PREFIX)) continue;
    const text = readText(f);
    if (text == null) continue;
    for (const n of names) {
      if (text.includes(n)) add(f, "real-nickname", "命中本地成员/CN隐私源中的称呼");
    }
  }

  // 5) config/app.json 内容泄露
  const leaks = collectConfigLeaks(configs, example, identifiers);
  for (const f of tracked) {
    if (f === SELF) continue;
    const text = readText(f);
    if (text == null) continue;
    for (const v of leaks) {
      if (text.includes(v)) add(f, "config-leak", "命中 config/app.json 的本地值");
    }
  }

  // 去重
  const seen = new Set();
  const deduped = [];
  for (const x of findings) {
    const key = `${x.file}|${x.kind}|${x.detail}`;
    if (seen.has(key)) continue;
    seen.add(key);
    deduped.push(x);
  }

  if (deduped.length === 0) {
    console.log(`privacy-scan: OK — ${tracked.length} 个跟踪文件，未发现隐私/密钥风险。`);
    return 0;
  }
  console.error(`privacy-scan: FAIL — 发现 ${deduped.length} 项风险：`);
  for (const x of deduped) {
    console.error(`  [${x.kind}] ${x.file} — ${x.detail}`);
  }
  return 1;
}

try {
  process.exit(main());
} catch {
  console.error("privacy-scan: FAIL — 无法完整读取 Git 索引或隐私源，未跳过检查；请在本机核对。");
  process.exit(1);
}
