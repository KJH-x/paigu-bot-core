import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '..', '..');
const EXE = path.join(
  REPO_ROOT,
  'target',
  'debug',
  process.platform === 'win32' ? 'paigu-bot-core.exe' : 'paigu-bot-core'
);
const FIXED_WINDOW = { start_ms: 1600000000000, end_ms: 1600003600000 };
const TARGET_IDENTITY = '成员01';
const BLOCKED_IDENTITY = '成员02';
const GROUP_ID = '123456789';

function log(line) {
  process.stdout.write(line + '\n');
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

function assertEq(actual, expected, msg) {
  if (actual !== expected) {
    throw new Error(`${msg}（期望 ${JSON.stringify(expected)}，实际 ${JSON.stringify(actual)}）`);
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function getFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

function buildBinary() {
  log('· cargo build ...');
  const result = spawnSync('cargo', ['build'], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
    timeout: 900000,
  });
  if (result.error) throw new Error('cargo build 启动失败: ' + result.error.message);
  if (result.status !== 0) throw new Error('cargo build 失败 (exit ' + result.status + ')');
  if (!fs.existsSync(EXE)) throw new Error('未找到二进制: ' + EXE);
}

function prepareConfig(tmpDir, gatewayPort) {
  const cfg = JSON.parse(fs.readFileSync(path.join(REPO_ROOT, 'config.example.json'), 'utf8'));
  cfg.gateway.bind = '127.0.0.1:' + gatewayPort;
  cfg.gateway.reply_enabled = false;
  cfg.llm.enabled = false;
  cfg.llm.fallback_to_rules = true;
  cfg.round.priority_window = { ...FIXED_WINDOW };
  cfg.round.priority_users = [TARGET_IDENTITY];
  cfg.members.cache_path = path.join(tmpDir, 'members.json');
  const cfgDir = path.join(tmpDir, 'config');
  fs.mkdirSync(cfgDir, { recursive: true });
  const cfgPath = path.join(cfgDir, 'app.json');
  fs.writeFileSync(cfgPath, JSON.stringify(cfg, null, 2), 'utf8');
  return cfgPath;
}

function startServer(port, cfgPath) {
  const child = spawn(EXE, ['run'], {
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      PAIGU_CONFIG_PATH: cfgPath,
      PAIGU_HTTP_PORT: String(port),
      PAIGU_WEB_DIR: path.join(REPO_ROOT, 'web'),
      PAIGU_MEMBERS_SEED_PATH: path.join(path.dirname(cfgPath), 'members.seed.json'),
      PAIGU_MEMBERS_EXAMPLE_PATH: path.join(REPO_ROOT, 'data', 'members.example.json'),
      RUST_LOG: process.env.RUST_LOG || 'warn',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let logs = '';
  const capture = (chunk) => {
    logs += chunk.toString();
    if (logs.length > 20000) logs = logs.slice(-20000);
  };
  child.stdout.on('data', capture);
  child.stderr.on('data', capture);
  return { child, logs: () => logs };
}

function startRemoteServer(snapshot) {
  return new Promise((resolve) => {
    const paths = [];
    const server = http.createServer((req, res) => {
      const pathname = decodeURIComponent((req.url || '/').split('?')[0]);
      paths.push(pathname);
      const headers = {
        'Content-Type': 'application/json',
        'Access-Control-Allow-Origin': '*',
      };
      if (pathname.endsWith('/current')) {
        res.writeHead(200, headers);
        res.end(JSON.stringify(snapshot));
      } else {
        res.writeHead(404, headers);
        res.end(JSON.stringify({ error: 'not_found' }));
      }
    });
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({
        base: `http://127.0.0.1:${port}`,
        paths,
        close: () => new Promise((done) => server.close(() => done())),
      });
    });
  });
}

function waitExit(child, timeoutMs) {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) return resolve(true);
    const timer = setTimeout(() => resolve(false), timeoutMs);
    child.once('exit', () => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

async function waitForHealth(base, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastError = '无响应';
  while (Date.now() < deadline) {
    try {
      const res = await fetch(base + '/api/health', { signal: AbortSignal.timeout(2000) });
      if (res.ok) {
        const body = await res.json();
        if (body.status === 'ok') return body;
      }
      lastError = 'HTTP ' + res.status;
    } catch (err) {
      lastError = err.message;
    }
    await sleep(200);
  }
  throw new Error('服务未就绪: ' + lastError);
}

async function waitForGateway(base, expected, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let last = '无响应';
  while (Date.now() < deadline) {
    try {
      const res = await fetch(base + '/api/gateway/status', { signal: AbortSignal.timeout(2000) });
      const body = await res.json();
      last = JSON.stringify(body);
      if (body.listening && (!expected || body.bound_addr === expected)) return body;
    } catch (err) {
      last = err.message;
    }
    await sleep(200);
  }
  throw new Error('Gateway 未就绪: ' + last);
}

async function api(base, method, urlPath, body) {
  const init = { method, signal: AbortSignal.timeout(10000) };
  if (body !== undefined) {
    init.headers = { 'Content-Type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const res = await fetch(base + urlPath, init);
  if (!res.ok) throw new Error(`${method} ${urlPath} -> HTTP ${res.status}`);
  return res.json();
}

function formatOffset(ms) {
  const sign = ms < 0 ? '-' : '+';
  let t = Math.floor(Math.abs(ms) / 1000);
  const dd = Math.floor(t / 86400);
  t -= dd * 86400;
  const hh = Math.floor(t / 3600);
  t -= hh * 3600;
  const mm = Math.floor(t / 60);
  t -= mm * 60;
  const pad = (n) => (n < 10 ? '0' : '') + n;
  return `${sign}${pad(dd)} ${pad(hh)} ${pad(mm)} ${pad(t)}`;
}

function firstSlotUser(board) {
  const allocations = board && Array.isArray(board.item_allocations) ? board.item_allocations : [];
  for (const alloc of allocations) {
    if (alloc.item_id === 'pass_sp' && alloc.variant_id === 'v_jcl') {
      const box = (alloc.boxes || [])[0];
      const slot = box && (box.slots || [])[0];
      return slot ? (slot.user_id ?? null) : null;
    }
  }
  return null;
}

function variantFirstSlotUser(board, variantId) {
  const allocations = board && Array.isArray(board.item_allocations) ? board.item_allocations : [];
  for (const alloc of allocations) {
    if (alloc.item_id === 'pass_sp' && alloc.variant_id === variantId) {
      const box = (alloc.boxes || [])[0];
      const slot = box && (box.slots || [])[0];
      return slot ? (slot.user_id ?? null) : null;
    }
  }
  return null;
}

async function waitForMessage(base, predicate, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const payload = await api(base, 'GET', '/api/display?since=0');
    const msgs = payload.messages || [];
    for (const m of msgs) {
      if (predicate(m)) return { message: m, board: payload.board, version: payload.version };
    }
    await sleep(150);
  }
  throw new Error('等待 /api/display 消息超时');
}

async function resetViaPage(page) {
  await page.click('#reset');
  await page.waitForFunction(() => {
    const b = document.querySelector('#reset');
    return !!b && !b.disabled;
  }, { timeout: 10000 });
  await sleep(150);
}

async function openSim(page, base, wsUrl) {
  const primary = `${base}/sim?api=${encodeURIComponent(base)}&ws=${encodeURIComponent(wsUrl)}`;
  await page.goto(primary, { waitUntil: 'domcontentloaded' });
  const loaded = await page
    .evaluate(() => typeof window.PAIGU !== 'undefined')
    .catch(() => false);
  if (loaded) return { url: primary, note: null };
  const fallback = `${base}/web/sim.html?api=${encodeURIComponent(base)}&ws=${encodeURIComponent(wsUrl)}`;
  await page.goto(fallback, { waitUntil: 'domcontentloaded' });
  return {
    url: fallback,
    note: `/sim 的相对静态资源返回 404（web/ 仅挂在 /web），已回退 ${fallback}`,
  };
}

async function sendViaUI(page, ctx, opts) {
  await page.selectOption('#identity-select', opts.identity);
  await page.waitForFunction(
    (identity) => {
      const el = document.querySelector('#identity-info');
      return !!el && el.textContent.includes(identity);
    },
    opts.identity,
    { timeout: 10000 }
  );
  await page.fill('#offset', opts.offsetText);
  await page.fill('#text', opts.text);
  await page.waitForFunction(() => window.__simWsReady === true, { timeout: 15000 });
  await page.click('#send');
  const found = await waitForMessage(
    ctx.base,
    (m) => m.text === opts.text && m.display === opts.identity,
    15000
  );
  await page.waitForFunction(
    (status) => {
      const outs = document.querySelectorAll('#transcript .out');
      if (!outs.length) return false;
      return (outs[outs.length - 1].textContent || '').includes(status);
    },
    opts.expectStatus,
    { timeout: 15000 }
  );
  return { outcome: { status: found.message.status, detail: found.message.detail }, board: found.board, message: found.message };
}

async function waitBoardFirstCell(page, expected) {
  await page.waitForFunction(
    (value) => {
      const cell = document.querySelector('#board .cell-user');
      return !!cell && cell.textContent.trim() === value;
    },
    expected,
    { timeout: 10000 }
  );
}

function runNode(args, input) {
  return spawnSync('node', args, {
    cwd: REPO_ROOT,
    input: input === undefined ? undefined : input,
    encoding: 'utf8',
    timeout: 120000,
  });
}

function stripVolatile(key, value) {
  if (key === 'claim_id' || key === 'segment_id' || key === 'generated_at') return undefined;
  return value;
}

function normalizeState(state) {
  const alloc = state.board && Array.isArray(state.board.item_allocations) ? state.board.item_allocations : [];
  const messages = (state.messages || []).map((m) => ({
    display: m.display,
    text: m.text,
    status: m.status,
    detail: m.detail,
  }));
  messages.sort((a, b) =>
    `${a.display}\u0000${a.text}\u0000${a.status}\u0000${a.detail}`
      .localeCompare(`${b.display}\u0000${b.text}\u0000${b.status}\u0000${b.detail}`)
  );
  return JSON.stringify({ alloc, messages }, stripVolatile);
}

function buildCases() {
  return [
    {
      name: 'WS 连接成功',
      fn: async (page, ctx) => {
        const status = await api(ctx.base, 'GET', '/api/gateway/status');
        assert(status.listening === true, 'Gateway 应处于监听: ' + JSON.stringify(status));
        assert(status.clients >= 1, 'Gateway 应有 WS 客户端: ' + JSON.stringify(status));
        const ready = await page.evaluate(() => window.__simWsReady === true);
        assert(ready, '页面 window.__simWsReady 应为 true');
        const badge = await page.textContent('#ws-badge');
        assert(badge.includes('WS 已连接'), 'WS 徽章应显示已连接，实际: ' + badge);
      },
    },
    {
      name: '常规排谷（经真实 WS）',
      fn: async (page, ctx) => {
        const r = await sendViaUI(page, ctx, {
          identity: TARGET_IDENTITY,
          offsetText: '+00 00 00 00',
          text: '排 通行证 结城理 1',
          expectStatus: 'Applied',
        });
        assertEq(r.outcome.status, 'Applied', '状态');
        assertEq(firstSlotUser(r.board), TARGET_IDENTITY, '排位表首格');
        await waitBoardFirstCell(page, TARGET_IDENTITY);
      },
    },
    {
      name: '非排谷忽略',
      fn: async (page, ctx) => {
        const r = await sendViaUI(page, ctx, {
          identity: TARGET_IDENTITY,
          offsetText: '+00 00 00 00',
          text: '今天天气不错',
          expectStatus: 'Ignored',
        });
        assertEq(r.outcome.status, 'Ignored', '状态');
      },
    },
    {
      name: '时段拒绝',
      fn: async (page, ctx) => {
        const offsetMs = Math.round(
          (FIXED_WINDOW.start_ms + FIXED_WINDOW.end_ms) / 2 - Date.now()
        );
        const r = await sendViaUI(page, ctx, {
          identity: BLOCKED_IDENTITY,
          offsetText: formatOffset(offsetMs),
          text: '排 通行证 结城理 1',
          expectStatus: 'Rejected',
        });
        assertEq(r.outcome.status, 'Rejected', '状态');
        assert(
          r.outcome.detail && (r.outcome.detail.includes('预存') || r.outcome.detail.includes('优先时段')),
          '拒绝原因应指向优先时段/预存，实际: ' + r.outcome.detail
        );
      },
    },
    {
      name: '预存优先',
      fn: async (page, ctx) => {
        const first = await sendViaUI(page, ctx, {
          identity: BLOCKED_IDENTITY,
          offsetText: '+00 00 00 00',
          text: '排 通行证 结城理 1',
          expectStatus: 'Applied',
        });
        assertEq(first.outcome.status, 'Applied', '非预存先排状态');
        assertEq(firstSlotUser(first.board), BLOCKED_IDENTITY, '非预存首格');
        const second = await sendViaUI(page, ctx, {
          identity: TARGET_IDENTITY,
          offsetText: '+00 00 00 00',
          text: '排 通行证 结城理 1',
          expectStatus: 'Applied',
        });
        assertEq(second.outcome.status, 'Applied', '预存排状态');
        assertEq(firstSlotUser(second.board), TARGET_IDENTITY, '预存应占首格');
        await waitBoardFirstCell(page, TARGET_IDENTITY);
      },
    },
    {
      name: 'scripts/sim-run --speed 0 回放',
      fn: async (page, ctx) => {
        const inputFile = path.join(ctx.tmpDir, 'run-input.jsonl');
        const wsUrl = ctx.wsUrl;
        const lines = [
          { ts: 1000, user_id: TARGET_IDENTITY, nickname: TARGET_IDENTITY, text: '排 通行证 结城理 1', offset_ms: 0, group_id: GROUP_ID },
          { ts: 5000, user_id: BLOCKED_IDENTITY, nickname: BLOCKED_IDENTITY, text: '排 通行证 岳羽由加莉 1', offset_ms: 0, group_id: GROUP_ID },
        ];
        fs.writeFileSync(inputFile, lines.map((l) => JSON.stringify(l)).join('\n') + '\n', 'utf8');
        const res = runNode([
          'scripts/sim-run.mjs',
          '--input', inputFile,
          '--ws', wsUrl,
          '--speed', '0',
          '--api', ctx.base,
          '--group', GROUP_ID,
        ]);
        assertEq(res.status, 0, 'sim-run 退出码 (stderr: ' + (res.stderr || '').trim() + ')');
        const summary = JSON.parse((res.stdout || '').trim());
        assertEq(summary.total, 2, 'sim-run total');
        assertEq(summary.sent, 2, 'sim-run sent');
        assertEq(summary.failed, 0, 'sim-run failed');
        assertEq(summary.statuses.Applied, 2, 'sim-run 应用状态统计');
        const m1 = await waitForMessage(ctx.base, (m) => m.text === '排 通行证 结城理 1' && m.display === TARGET_IDENTITY, 10000);
        await waitForMessage(ctx.base, (m) => m.text === '排 通行证 岳羽由加莉 1' && m.display === BLOCKED_IDENTITY, 10000);
        assertEq(variantFirstSlotUser(m1.board, 'v_jcl'), TARGET_IDENTITY, 'run 回放首格');
      },
    },
    {
      name: '录制→重放一致',
      fn: async (page, ctx) => {
        const recordFile = path.join(ctx.tmpDir, 'record.jsonl');
        const wsUrl = ctx.wsUrl;
        const scripted = [
          'id ' + TARGET_IDENTITY + ' ' + TARGET_IDENTITY,
          '排 通行证 结城理 1',
          'id ' + BLOCKED_IDENTITY + ' ' + BLOCKED_IDENTITY,
          '排 通行证 岳羽由加莉 1',
        ].join('\n') + '\n';
        const rec = runNode(
          ['scripts/sim-record.mjs', '--record', recordFile, '--ws', wsUrl, '--group', GROUP_ID],
          scripted
        );
        assertEq(rec.status, 0, '录制退出码 (stderr: ' + (rec.stderr || '').trim() + ')');
        const recorded = fs.readFileSync(recordFile, 'utf8').trim().split(/\r?\n/).filter(Boolean);
        assertEq(recorded.length, 2, 'record.jsonl 行数');
        const firstRecord = JSON.parse(recorded[0]);
        for (const key of ['ts', 'user_id', 'nickname', 'text', 'offset_ms', 'group_id']) {
          assert(Object.prototype.hasOwnProperty.call(firstRecord, key), 'record 字段缺失: ' + key);
        }
        await waitForMessage(ctx.base, (m) => m.text === '排 通行证 岳羽由加莉 1', 10000);
        const stateA = await api(ctx.base, 'GET', '/api/display?since=0');
        const normalizedA = normalizeState(stateA);

        await resetViaPage(page);

        const rep = runNode([
          'scripts/sim-record.mjs',
          '--replay', recordFile,
          '--ws', wsUrl,
          '--speed', '0',
          '--api', ctx.base,
          '--group', GROUP_ID,
        ]);
        assertEq(rep.status, 0, '重放退出码 (stderr: ' + (rep.stderr || '').trim() + ')');
        await waitForMessage(ctx.base, (m) => m.text === '排 通行证 岳羽由加莉 1', 10000);
        const stateB = await api(ctx.base, 'GET', '/api/display?since=0');
        const normalizedB = normalizeState(stateB);
        assertEq(normalizedB, normalizedA, '录制与重放结果应一致');
      },
    },
    {
      name: '/sim 静态资源 200',
      fn: async (page, ctx) => {
        for (const urlPath of ['/sim', '/common.js', '/sim.js', '/display.css']) {
          const res = await fetch(ctx.base + urlPath, { signal: AbortSignal.timeout(10000) });
          assertEq(res.status, 200, urlPath + ' 状态');
        }
      },
    },
    {
      name: 'PUT /api/config 冲突 409',
      fn: async (page, ctx) => {
        const current = await fetch(ctx.base + '/api/config', {
          signal: AbortSignal.timeout(10000),
        }).then((res) => res.json());
        const res = await fetch(ctx.base + '/api/config', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ config: current.config, revision: (current.revision || 0) + 999 }),
          signal: AbortSignal.timeout(10000),
        });
        assertEq(res.status, 409, '陈旧 revision 状态');
        const body = await res.json();
        assertEq(body.error, 'stale_revision', '错误码');
      },
    },
    {
      name: '/api/members 回退 example',
      fn: async (page, ctx) => {
        const res = await fetch(ctx.base + '/api/members', {
          signal: AbortSignal.timeout(10000),
        }).then((r) => r.json());
        assertEq(res.source, 'example', '成员来源');
        assertEq(res.members.length, 5, '占位成员数');
        assert(
          res.members.every((m) => String(m.nickname || '').startsWith('成员')),
          '占位昵称应以「成员」开头'
        );
      },
    },
    {
      name: 'remote 数据源',
      fn: async (page, ctx) => {
        const url =
          `${ctx.base}/?api=${encodeURIComponent(ctx.base)}` +
          `&source=remote&remote=${encodeURIComponent(ctx.remote.base)}` +
          `&round=${encodeURIComponent('月行水上')}`;
        await page.goto(url, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(
          () => {
            const cell = document.querySelector('#board .cell-user');
            return !!cell && cell.textContent.trim() === '远程用户';
          },
          { timeout: 15000 }
        );
        const hit = ctx.remote.paths.find((p) => p.endsWith('/current'));
        assert(hit, 'remote 应请求 rounds/<id>/current');
        assert(!hit.includes('.json'), 'remote 路径不应带 .json: ' + hit);
      },
    },
  ];
}

async function main() {
  buildBinary();

  const port = await getFreePort();
  const gatewayPort = await getFreePort();
  const base = `http://127.0.0.1:${port}`;
  const wsUrl = `ws://127.0.0.1:${gatewayPort}`;
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'paigu-e2e-'));
  const cfgPath = prepareConfig(tmpDir, gatewayPort);

  let child = null;
  let serverLogs = () => '';
  let browser = null;
  let remote = null;
  let failures = 0;
  const cases = buildCases();

  try {
    const server = startServer(port, cfgPath);
    child = server.child;
    serverLogs = server.logs;

    const health = await waitForHealth(base, 40000);
    log(`· 服务就绪 ${base} (version ${health.version})`);
    const gw = await waitForGateway(base, `127.0.0.1:${gatewayPort}`, 20000);
    log(`· Gateway 就绪 ${gw.bound_addr}`);

    remote = await startRemoteServer({
      round_id: '月行水上',
      title: '月行水上',
      version: 7,
      updated_at: '2026-09-07 22:00',
      item_allocations: [
        {
          item_id: 'pass_sp',
          variant_id: 'v_jcl',
          boxes: [
            { box_index: 1, slots: [{ slot_index: 1, user_id: '远程用户', status: 'filled' }] },
          ],
        },
      ],
    });
    const ctx = { base, remote, wsUrl, tmpDir };

    browser = await chromium.launch();
    const page = await browser.newPage();
    page.setDefaultTimeout(15000);

    const opened = await openSim(page, base, wsUrl);
    if (opened.note) log('NOTE ' + opened.note);

    await page.waitForFunction(
      () => {
        const select = document.querySelector('#identity-select');
        return !!select && select.options.length > 3;
      },
      { timeout: 15000 }
    );
    await page.waitForFunction(() => window.__simWsReady === true, { timeout: 15000 });
    await page.evaluate(() => {
      try {
        Object.defineProperty(document, 'hidden', { configurable: true, get: () => true });
      } catch (err) {
      }
    });

    for (const testCase of cases) {
      if (testCase.name !== 'remote 数据源') {
        await resetViaPage(page);
      }
      try {
        await testCase.fn(page, ctx);
        log(`PASS ${testCase.name}`);
      } catch (err) {
        failures += 1;
        log(`FAIL ${testCase.name} ${err && err.message ? err.message : err}`);
      }
    }
  } catch (err) {
    failures += 1;
    log('FAIL setup ' + (err && err.message ? err.message : err));
    const tail = serverLogs().split(/\r?\n/).filter(Boolean).slice(-20).join('\n');
    if (tail) log('--- server log tail ---\n' + tail);
  } finally {
    try {
      if (browser) await browser.close();
    } catch (err) {
    }
    try {
      if (remote) await remote.close();
    } catch (err) {
    }
    if (child) {
      try {
        child.kill();
      } catch (err) {
      }
      const exited = await waitExit(child, 5000);
      if (!exited) {
        try {
          child.kill('SIGKILL');
        } catch (err) {
          }
      }
    }
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (err) {
    }
  }

  log(failures === 0 ? `ALL PASS (${cases.length} cases)` : `${failures} case(s) failed`);
  process.exitCode = failures === 0 ? 0 : 1;
}

main();
