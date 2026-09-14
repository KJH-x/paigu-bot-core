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

function prepareConfig(tmpDir) {
  const cfg = JSON.parse(fs.readFileSync(path.join(REPO_ROOT, 'config.example.json'), 'utf8'));
  cfg.gateway.bind = '127.0.0.1:0';
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

async function openSim(page, base) {
  const primary = `${base}/sim?api=${encodeURIComponent(base)}`;
  await page.goto(primary, { waitUntil: 'domcontentloaded' });
  const loaded = await page
    .evaluate(() => typeof window.PAIGU !== 'undefined')
    .catch(() => false);
  if (loaded) return { url: primary, note: null };
  const fallback = `${base}/web/sim.html?api=${encodeURIComponent(base)}`;
  await page.goto(fallback, { waitUntil: 'domcontentloaded' });
  return {
    url: fallback,
    note: `/sim 的相对静态资源返回 404（web/ 仅挂在 /web），已回退 ${fallback}`,
  };
}

async function sendViaUI(page, opts) {
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
  const [response] = await Promise.all([
    page.waitForResponse(
      (res) => res.url().includes('/api/sim/message') && res.request().method() === 'POST',
      { timeout: 15000 }
    ),
    page.click('#send'),
  ]);
  const payload = await response.json();
  await page.waitForFunction(
    (status) => {
      const outs = document.querySelectorAll('#transcript .out');
      if (!outs.length) return false;
      return (outs[outs.length - 1].textContent || '').includes(status);
    },
    opts.expectStatus,
    { timeout: 10000 }
  );
  return payload;
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

function buildCases() {
  return [
    {
      name: '常规排谷',
      fn: async (page) => {
        const r = await sendViaUI(page, {
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
      fn: async (page) => {
        const r = await sendViaUI(page, {
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
      fn: async (page) => {
        const offsetMs = Math.round(
          (FIXED_WINDOW.start_ms + FIXED_WINDOW.end_ms) / 2 - Date.now()
        );
        const r = await sendViaUI(page, {
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
      fn: async (page) => {
        const first = await sendViaUI(page, {
          identity: BLOCKED_IDENTITY,
          offsetText: '+00 00 00 00',
          text: '排 通行证 结城理 1',
          expectStatus: 'Applied',
        });
        assertEq(first.outcome.status, 'Applied', '非预存先排状态');
        assertEq(firstSlotUser(first.board), BLOCKED_IDENTITY, '非预存首格');
        const second = await sendViaUI(page, {
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
  const base = `http://127.0.0.1:${port}`;
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'paigu-e2e-'));
  const cfgPath = prepareConfig(tmpDir);

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
    const ctx = { base, remote };

    browser = await chromium.launch();
    const page = await browser.newPage();
    page.setDefaultTimeout(15000);

    const opened = await openSim(page, base);
    if (opened.note) log('NOTE ' + opened.note);

    await page.waitForFunction(
      () => {
        const select = document.querySelector('#identity-select');
        return !!select && select.options.length > 3;
      },
      { timeout: 15000 }
    );
    await page.evaluate(() => {
      try {
        Object.defineProperty(document, 'hidden', { configurable: true, get: () => true });
      } catch (err) {
      }
    });

    for (const testCase of cases) {
      await api(base, 'POST', '/api/sim/reset', {});
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
