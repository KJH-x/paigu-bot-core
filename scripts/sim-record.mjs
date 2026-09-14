#!/usr/bin/env node
import fs from 'node:fs';
import readline from 'node:readline';
import {
  readJsonl,
  appendJsonl,
  connectWs,
  sendJson,
  buildEvent,
  parseOffset,
  replayEntries,
  fetchJson,
  tallyStatuses,
  parseArgs,
  makeId,
  sleep
} from './sim-lib.mjs';

const DEFAULT_WS = 'ws://127.0.0.1:9801';

function usage() {
  console.log(`用法:
  # 录制：从 stdin 读取操作序列，写入 record.jsonl（逐行 JSON）
  node scripts/sim-record.mjs --record <file.jsonl> [--ws <url>] [--group <id>]

  # 重放：按 record.jsonl 自动重放
  node scripts/sim-record.mjs --replay <file.jsonl> [--ws <url>] [--speed <N>] [--api <base>]

录制命令（每行一条）:
  id <user_id> [nickname...]   设置当前身份（昵称默认取 user_id）
  nick <nickname>              只改昵称
  offset <±DD HH MM SS>        设置时间偏移
  group <group_id>             设置群号
  admin / noadmin              切换管理员标记
  text <message>               发送消息
  <其它任意文本>                直接作为消息发送
  # 注释 / 空行                忽略
  quit / exit                  结束

记录字段: {ts,user_id,nickname,text,offset_ms,group_id}
`);
}

async function runRecord(args) {
  const file = args.record;
  const wsUrl = typeof args.ws === 'string' ? args.ws : DEFAULT_WS;
  const defaultGroup = typeof args.group === 'string' ? args.group : '720675572';

  let ws;
  try {
    ws = await connectWs(wsUrl);
  } catch (err) {
    console.error('无法连接 WS: ' + err.message);
    return 1;
  }

  fs.writeFileSync(file, '', 'utf8');

  const session = {
    user_id: '成员01',
    nickname: '成员01',
    offset_ms: 0,
    group_id: defaultGroup,
    is_admin: false
  };
  let count = 0;

  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const raw of rl) {
    const line = String(raw).trim();
    if (!line || line.startsWith('#')) continue;
    if (line === 'quit' || line === 'exit') break;

    if (line.startsWith('id ')) {
      const rest = line.slice(3).trim().split(/\s+/);
      session.user_id = rest[0];
      session.nickname = rest.slice(1).join(' ') || rest[0];
      continue;
    }
    if (line.startsWith('nick ')) { session.nickname = line.slice(5).trim(); continue; }
    if (line.startsWith('group ')) { session.group_id = line.slice(6).trim(); continue; }
    if (line === 'admin') { session.is_admin = true; continue; }
    if (line === 'noadmin') { session.is_admin = false; continue; }
    if (line.startsWith('offset ')) {
      const ms = parseOffset(line.slice(7).trim());
      if (ms === null) { console.error('偏移格式无效: ' + line); continue; }
      session.offset_ms = ms;
      continue;
    }

    const text = line.startsWith('text ') ? line.slice(5) : line;
    const record = {
      ts: Date.now(),
      user_id: session.user_id,
      nickname: session.nickname,
      text,
      offset_ms: session.offset_ms,
      group_id: session.group_id
    };
    appendJsonl(file, record);

    const event = buildEvent({
      user_id: record.user_id,
      nickname: record.nickname,
      text: record.text,
      group_id: record.group_id,
      offset_ms: record.offset_ms,
      is_admin: session.is_admin,
      message_id: makeId('rec')
    });
    try {
      sendJson(ws, event);
      count += 1;
      console.log(`recorded #${count} ${record.nickname}: ${text}`);
    } catch (err) {
      console.error('发送失败: ' + err.message);
    }
  }

  try { ws.close(); } catch (err) { /* ignore */ }
  console.log(JSON.stringify({ mode: 'record', file, recorded: count, ws: wsUrl }, null, 2));
  return 0;
}

async function runReplay(args) {
  const file = args.replay;
  const wsUrl = typeof args.ws === 'string' ? args.ws : DEFAULT_WS;
  const speed = args.speed === undefined ? 1 : Number(args.speed);
  if (!Number.isFinite(speed) || speed < 0) { console.error('--speed 必须 >= 0'); return 2; }
  const apiBase = typeof args.api === 'string' ? args.api.replace(/\/+$/, '') : '';

  const entries = readJsonl(file);
  if (!entries.length) { console.error('记录为空: ' + file); return 2; }

  let ws;
  try {
    ws = await connectWs(wsUrl);
  } catch (err) {
    console.error('无法连接 WS: ' + err.message);
    return 1;
  }

  const stats = await replayEntries(entries, {
    ws,
    speed,
    groupId: typeof args.group === 'string' ? args.group : '',
    log: (line) => console.log(line)
  });

  let statuses = {};
  let version = null;
  if (apiBase) {
    try {
      await sleep(400);
      const payload = await fetchJson(apiBase + '/api/display?since=0');
      version = payload.version;
      statuses = tallyStatuses(payload.messages, entries);
    } catch (err) {
      console.error('拉取 /api/display 失败: ' + err.message);
    }
  }

  try { ws.close(); } catch (err) { /* ignore */ }
  console.log(JSON.stringify({
    mode: 'replay',
    file,
    ws: wsUrl,
    speed,
    total: stats.total,
    sent: stats.sent,
    failed: stats.failed,
    elapsed_ms: stats.elapsed_ms,
    statuses,
    version
  }, null, 2));
  return stats.failed > 0 ? 1 : 0;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || (!args.record && !args.replay)) { usage(); return args.help ? 0 : 2; }
  if (args.record) return runRecord(args);
  return runReplay(args);
}

main().then((code) => { process.exitCode = code; }).catch((err) => {
  console.error(err && err.stack ? err.stack : String(err));
  process.exitCode = 1;
});
