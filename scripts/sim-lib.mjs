import fs from 'node:fs';

let idCounter = 0;

export function makeId(prefix) {
  idCounter += 1;
  return `${prefix || 'sim'}-${Date.now()}-${idCounter}-${Math.floor(Math.random() * 1e6)}`;
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function parseOffset(str) {
  if (str == null) return 0;
  let s = String(str).trim();
  if (!s) return 0;
  let sign = 1;
  if (s.charAt(0) === '-') { sign = -1; s = s.slice(1).trim(); }
  else if (s.charAt(0) === '+') { s = s.slice(1).trim(); }
  const parts = s.split(/[\s:]+/).filter((x) => x !== '');
  if (!parts.length || parts.length > 4) return null;
  const nums = [];
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return null;
    nums.push(parseInt(p, 10));
  }
  while (nums.length < 4) nums.unshift(0);
  const [dd, hh, mm, ss] = nums;
  if (mm > 59 || ss > 59) return null;
  return sign * (((dd * 24 + hh) * 60 + mm) * 60 + ss) * 1000;
}

export function formatOffset(ms) {
  const sign = ms < 0 ? '-' : '+';
  let t = Math.abs(Math.floor(ms / 1000));
  const dd = Math.floor(t / 86400); t -= dd * 86400;
  const hh = Math.floor(t / 3600); t -= hh * 3600;
  const mm = Math.floor(t / 60); t -= mm * 60;
  const pad = (n) => (n < 10 ? '0' : '') + n;
  return `${sign}${pad(dd)} ${pad(hh)} ${pad(mm)} ${pad(t)}`;
}

export function buildEvent(opts) {
  const offsetMs = Number.isFinite(opts.offset_ms) ? opts.offset_ms : 0;
  const uid = String(opts.user_id);
  const nick = opts.nickname == null ? uid : String(opts.nickname);
  const groupId = opts.group_id == null ? '' : String(opts.group_id);
  return {
    post_type: 'message',
    message_type: 'group',
    self_id: opts.self_id != null ? opts.self_id : 3000000000,
    user_id: uid,
    group_id: groupId,
    time: Math.floor((Date.now() + offsetMs) / 1000),
    message_id: String(opts.message_id || makeId('sim')),
    raw_message: String(opts.text == null ? '' : opts.text),
    message: [{ type: 'text', data: { text: String(opts.text == null ? '' : opts.text) } }],
    sender: { user_id: uid, nickname: nick, card: nick, role: opts.is_admin ? 'admin' : 'member' }
  };
}

export function readJsonl(file) {
  const text = fs.readFileSync(file, 'utf8');
  const out = [];
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line || line.startsWith('#')) continue;
    try {
      out.push(JSON.parse(line));
    } catch (err) {
      throw new Error(`第 ${i + 1} 行不是合法 JSON: ${err.message}`);
    }
  }
  return out;
}

export function appendJsonl(file, obj) {
  fs.appendFileSync(file, JSON.stringify(obj) + '\n', 'utf8');
}

export function connectWs(url, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    let ws;
    try {
      ws = new WebSocket(url);
    } catch (err) {
      reject(new Error(`WS 地址无效 ${url}: ${err.message}`));
      return;
    }
    const timer = setTimeout(() => {
      try { ws.close(); } catch (err) { /* ignore */ }
      reject(new Error(`WS 连接超时: ${url}`));
    }, timeoutMs);
    ws.onopen = () => {
      clearTimeout(timer);
      resolve(ws);
    };
    ws.onerror = () => {
      clearTimeout(timer);
      reject(new Error(`WS 连接失败: ${url}`));
    };
    ws.onclose = () => {
      clearTimeout(timer);
      reject(new Error(`WS 连接关闭: ${url}`));
    };
  });
}

export function sendJson(ws, obj) {
  if (!ws || ws.readyState !== 1) throw new Error('WS 未连接，无法发送');
  ws.send(JSON.stringify(obj));
}

export async function fetchJson(url) {
  const res = await fetch(url, { signal: AbortSignal.timeout(10000) });
  if (!res.ok) throw new Error(`GET ${url} -> HTTP ${res.status}`);
  return res.json();
}

export async function replayEntries(entries, opts) {
  const { ws = null, speed = 1, dryRun = false, groupId = '', log = () => {} } = opts;
  const started = Date.now();
  const stats = { total: entries.length, sent: 0, failed: 0, elapsed_ms: 0 };
  const baseTs = pickBase(entries, 'ts');
  const baseOffset = pickBase(entries, 'offset_ms');

  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (speed > 0) {
      let target = 0;
      if (baseTs != null && Number.isFinite(entry.ts)) {
        target = (entry.ts - baseTs) / speed;
      } else if (baseOffset != null && Number.isFinite(entry.offset_ms)) {
        target = (entry.offset_ms - baseOffset) / speed;
      }
      const wait = target - (Date.now() - started);
      if (wait > 0) await sleep(wait);
    }

    const event = buildEvent({
      user_id: entry.user_id,
      nickname: entry.nickname,
      text: entry.text,
      group_id: entry.group_id || groupId,
      offset_ms: Number.isFinite(entry.offset_ms) ? entry.offset_ms : 0,
      is_admin: !!entry.is_admin,
      message_id: entry.message_id || makeId('replay')
    });

    if (dryRun) {
      log(JSON.stringify(event));
      stats.sent += 1;
      continue;
    }
    try {
      sendJson(ws, event);
      stats.sent += 1;
    } catch (err) {
      stats.failed += 1;
      log(`发送失败: ${err.message}`);
    }
    await sleep(5);
  }

  stats.elapsed_ms = Date.now() - started;
  return stats;
}

function pickBase(entries, key) {
  for (const entry of entries) {
    if (Number.isFinite(entry[key])) return entry[key];
  }
  return null;
}

export function tallyStatuses(messages, entries) {
  const wanted = new Set(entries.map((e) => String(e.text)));
  const counts = {};
  for (const m of messages || []) {
    if (!wanted.has(String(m.text))) continue;
    const status = m.status || 'unknown';
    counts[status] = (counts[status] || 0) + 1;
  }
  return counts;
}

export function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith('--')) { out._.push(arg); continue; }
    const eq = arg.indexOf('=');
    if (eq >= 0) {
      out[arg.slice(2, eq)] = arg.slice(eq + 1);
    } else {
      const key = arg.slice(2);
      const next = argv[i + 1];
      if (next === undefined || next.startsWith('--')) out[key] = true;
      else { out[key] = next; i += 1; }
    }
  }
  return out;
}
