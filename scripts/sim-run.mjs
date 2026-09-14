#!/usr/bin/env node
import {
  readJsonl,
  connectWs,
  replayEntries,
  fetchJson,
  tallyStatuses,
  parseArgs,
  sleep
} from './sim-lib.mjs';

const DEFAULT_WS = 'ws://127.0.0.1:9801';

function usage() {
  console.log(`用法: node scripts/sim-run.mjs --input <file.jsonl> [选项]

按 --speed 倍率，把 JSONL 中的消息按真实相对时间经 WS 发送到同一 server。

选项:
  --input <file>   JSONL 输入；每行 {user_id,nickname,text,ts?,offset_ms?,group_id?}
  --ws <url>       WS 地址（默认 ${DEFAULT_WS}）
  --speed <N>      时间倍率（默认 1；0 = 尽快发送，不等待）
  --group <id>     默认群号（行内 group_id 优先）
  --api <base>     可选；结束后拉 /api/display 统计各消息状态
  --dry-run        只打印将发送的事件，不建立 WS 连接
  --help           显示帮助
`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) { usage(); return 0; }

  const input = typeof args.input === 'string' ? args.input : '';
  if (!input) { console.error('缺少 --input <file.jsonl>'); usage(); return 2; }

  const wsUrl = typeof args.ws === 'string' ? args.ws : DEFAULT_WS;
  const speed = args.speed === undefined ? 1 : Number(args.speed);
  if (!Number.isFinite(speed) || speed < 0) { console.error('--speed 必须 >= 0'); return 2; }
  const groupId = typeof args.group === 'string' ? args.group : '';
  const apiBase = typeof args.api === 'string' ? args.api.replace(/\/+$/, '') : '';
  const dryRun = !!args['dry-run'];

  const entries = readJsonl(input);
  if (!entries.length) { console.error('输入为空: ' + input); return 2; }

  let ws = null;
  if (!dryRun) {
    try {
      ws = await connectWs(wsUrl);
    } catch (err) {
      console.error('无法连接 WS: ' + err.message);
      return 1;
    }
  }

  const stats = await replayEntries(entries, {
    ws,
    speed,
    dryRun,
    groupId,
    log: (line) => console.log(line)
  });

  let statuses = {};
  let version = null;
  if (apiBase && !dryRun) {
    try {
      await sleep(400);
      const payload = await fetchJson(apiBase + '/api/display?since=0');
      version = payload.version;
      statuses = tallyStatuses(payload.messages, entries);
    } catch (err) {
      console.error('拉取 /api/display 失败: ' + err.message);
    }
  }

  if (ws) { try { ws.close(); } catch (err) { /* ignore */ } }

  const summary = {
    input,
    ws: dryRun ? null : wsUrl,
    speed,
    dry_run: dryRun,
    total: stats.total,
    sent: stats.sent,
    failed: stats.failed,
    elapsed_ms: stats.elapsed_ms,
    statuses,
    version
  };
  console.log(JSON.stringify(summary, null, 2));
  return stats.failed > 0 ? 1 : 0;
}

main().then((code) => { process.exitCode = code; }).catch((err) => {
  console.error(err && err.stack ? err.stack : String(err));
  process.exitCode = 1;
});
