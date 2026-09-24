#!/usr/bin/env node
// 隐私/密钥扫描：只扫描 git 跟踪文件（git ls-files），命中即报错（退出码非 0）。
// 本脚本不得包含任何真实昵称/群号/内网 IP 字面量；仅使用通用模式。
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// 以脚本位置定位仓库根（不依赖 CWD）
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const SELF = "scripts/privacy-scan.mjs";
const PLACEHOLDER_NICK = /^成员\d{2}$/;
const ALLOWED_DATA = new Set(["data/members.example.json"]);

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
  const out = execFileSync("git", ["-c", "core.quotepath=false", "ls-files"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
  });
  return out
    .split("\n")
    .map((s) => s.replace(/\r$/, ""))
    .filter((s) => s.length > 0);
}

function readText(rel) {
  const abs = path.join(REPO_ROOT, rel);
  let buf;
  try {
    buf = readFileSync(abs);
  } catch {
    return null;
  }
  if (buf.includes(0)) return null; // 二进制
  return buf.toString("utf8");
}

function collectNicknames(rel) {
  const abs = path.join(REPO_ROOT, rel);
  if (!existsSync(abs)) return [];
  let data;
  try {
    data = JSON.parse(readFileSync(abs, "utf8"));
  } catch {
    return [];
  }
  const list = Array.isArray(data) ? data : Array.isArray(data.members) ? data.members : [];
  const names = new Set();
  for (const m of list) {
    const n = m && typeof m.nickname === "string" ? m.nickname.trim() : "";
    if (n.length >= 2 && !PLACEHOLDER_NICK.test(n) && isDistinctiveNickname(n)) names.add(n);
  }
  return [...names];
}

function flattenStrings(value, out) {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => flattenStrings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => flattenStrings(v, out));
}

function collectConfigLeaks() {
  const localPath = path.join(REPO_ROOT, "config", "app.json");
  const examplePath = path.join(REPO_ROOT, "config.example.json");
  if (!existsSync(localPath)) return [];
  let local, example = {};
  try {
    local = JSON.parse(readFileSync(localPath, "utf8"));
  } catch {
    return [];
  }
  try {
    example = JSON.parse(readFileSync(examplePath, "utf8"));
  } catch {
    example = {};
  }
  const exampleArr = [];
  flattenStrings(example, exampleArr);
  const exampleStrings = new Set(exampleArr);

  const localStrings = [];
  flattenStrings(local, localStrings);

  const distinctive = new Set();
  for (const v of localStrings) {
    if (exampleStrings.has(v)) continue;
    const t = v.trim();
    if (/^\d{6,}$/.test(t)) distinctive.add(t); // 群号 / user_id
    else if (/^[A-Za-z0-9_./-]{20,}$/.test(t) && /[0-9]/.test(t)) distinctive.add(t); // token-like
  }
  return [...distinctive];
}

function main() {
  const tracked = gitLsFiles();
  const findings = [];
  const add = (file, kind, detail) => findings.push({ file, kind, detail });

  // 1) 禁止入库路径
  for (const f of tracked) {
    if (f.endsWith(".xlsx") || f.endsWith(".xls")) add(f, "tracked-spreadsheet", "*.xlsx/*.xls 不得入库");
    if (f.startsWith("config/")) add(f, "tracked-config", "config/** 不得入库");
    if (f.startsWith("data/") && !ALLOWED_DATA.has(f)) add(f, "tracked-data", "data/**（除 members.example.json）不得入库");
  }

  // 2) 密钥形态
  for (const f of tracked) {
    if (f === SELF) continue;
    const text = readText(f);
    if (text == null) continue;
    for (const p of SECRET_PATTERNS) {
      if (p.re.test(text)) add(f, "secret", p.name);
    }
  }

  // 3) 内网 IP
  for (const f of tracked) {
    if (f === SELF) continue;
    const text = readText(f);
    if (text == null) continue;
    for (const p of IP_PATTERNS) {
      if (p.re.test(text)) add(f, "private-ip", p.name);
    }
  }

  // 4) 真实昵称（data/members.json / seed）
  const nickSources = ["data/members.json", "data/members.seed.json"];
  const names = [];
  for (const s of nickSources) names.push(...collectNicknames(s));
  const uniqueNames = [...new Set(names)];
  for (const f of tracked) {
    if (f === SELF) continue;
    const text = readText(f);
    if (text == null) continue;
    for (const n of uniqueNames) {
      if (text.includes(n)) add(f, "real-nickname", `命中 data/members.json 中的真实昵称`);
    }
  }

  // 5) config/app.json 内容泄露
  const leaks = collectConfigLeaks();
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

process.exit(main());
