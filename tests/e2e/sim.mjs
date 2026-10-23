import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { verifyBoardLayout } from './board-layout.mjs';
import { verifyMessageIdentity } from './message-identity.mjs';
import { verifyAutomaticBaseline } from './automatic-baseline.mjs';
import { verifyMessageActions } from './message-actions.mjs';
import { verifyMessageTransactions } from './message-transactions.mjs';
import { verifyCorrectnessGates } from './correctness-gates.mjs';
import { verifyWorkspaceNavigation } from './workspace-navigation.mjs';
import { verifyScopedSettings } from './scoped-settings.mjs';
import { verifyPolicyReturn } from './message-policy-return.mjs';
import { verifyMemberBoundaries } from './member-boundaries.mjs';
import { verifyMemberPullHealth } from './member-pull.mjs';
import { verifyProxyAttribution } from './proxy-attribution.mjs';
import { verifyTargetModify } from './target-modify.mjs';
import { verifyPrivateBackup } from './private-backup.mjs';
import { verifySettlementReview } from './settlement-review.mjs';
import { verifyReplayGraph } from './replay-graph.mjs';
import { verifyDraftConcurrency } from './draft-concurrency.mjs';
import { verifyCatalogReachability } from './catalog-reachability.mjs';

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
  // 测试轮隔离：本会话为测试轮，绑定测试群（= 配置里的群号）。
  cfg.gateway.test_group_id = GROUP_ID;
  cfg.round.category = 'test';
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
      PAIGU_ROUNDS_DIR: path.join(path.dirname(cfgPath), 'rounds'),
      PAIGU_MESSAGES_DIR: path.join(path.dirname(cfgPath), 'messages'),
      PAIGU_EVENTS_DIR: path.join(path.dirname(cfgPath), 'events'),
      PAIGU_SETTLEMENT_DIR: path.join(path.dirname(cfgPath), 'settlement'),
      PAIGU_SNAPSHOT_DIR: path.join(path.dirname(cfgPath), 'snapshots'),
      PAIGU_COMPUTED_DIR: path.join(path.dirname(cfgPath), 'computed'),
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

function firstSlotUser(board) {
  const allocations = board && Array.isArray(board.item_allocations) ? board.item_allocations : [];
  for (const alloc of allocations) {
    if (alloc.item_id === 'pass_sp' && alloc.subitem_id === 'v_jcl') {
      const box = (alloc.boxes || [])[0];
      const slot = box && (box.slots || [])[0];
      return slot ? (slot.user_id ?? null) : null;
    }
  }
  return null;
}

function subitemFirstSlotUser(board, subitemId) {
  const allocations = board && Array.isArray(board.item_allocations) ? board.item_allocations : [];
  for (const alloc of allocations) {
    if (alloc.item_id === 'pass_sp' && alloc.subitem_id === subitemId) {
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

/** 重置（手动，HTTP）：sim 页已移除。 */
async function resetViaHttp(base) {
  await api(base, 'POST', '/api/sim/reset', {});
}

/**
 * 测试注入（仅测试轮）：`POST /api/sim/messages`（批量，逐条处理）。
 * 每条消息自带 `timestamp`；缺字段/非测试群由服务端逐条 `Dropped`。
 */
async function injectMessages(base, messages) {
  return api(base, 'POST', '/api/sim/messages', messages);
}

/** 单条注入的便捷封装（对齐旧 `sendViaUI` 语义）。 */
async function injectViaApi(base, opts) {
  const timestamp = opts.timestamp != null ? opts.timestamp : Date.now() + (opts.offsetMs || 0);
  const res = await injectMessages(base, [
    {
      user_id: opts.user_id || opts.identity,
      nickname: opts.nickname || opts.identity,
      text: opts.text,
      timestamp,
      group_id: GROUP_ID,
    },
  ]);
  const r = res.results[0];
  return {
    outcome: { status: r.status, detail: r.detail },
    board: res.board,
    message: r,
  };
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
      name: 'Gateway 监听',
      fn: async (page, ctx) => {
        const status = await api(ctx.base, 'GET', '/api/gateway/status');
        assert(status.listening === true, 'Gateway 应处于监听: ' + JSON.stringify(status));
      },
    },
    {
      name: '常规排谷（HTTP 注入）',
      fn: async (page, ctx) => {
        const r = await injectViaApi(ctx.base, {
          identity: TARGET_IDENTITY,
          text: '排 通行证 结城理 1',
        });
        assertEq(r.outcome.status, 'Applied', '状态');
        assertEq(firstSlotUser(r.board), TARGET_IDENTITY, '排位表首格');
        const found = await waitForMessage(
          ctx.base,
          (m) => m.text === '排 通行证 结城理 1' && m.display === TARGET_IDENTITY,
          10000
        );
        assertEq(found.message.status, 'Applied', '消息状态');
      },
    },
    {
      name: '非排谷忽略',
      fn: async (page, ctx) => {
        const r = await injectViaApi(ctx.base, {
          identity: TARGET_IDENTITY,
          text: '今天天气不错',
        });
        assertEq(r.outcome.status, 'Ignored', '状态');
      },
    },
    {
      name: '时段拒绝',
      fn: async (page, ctx) => {
        const timestamp = Math.round((FIXED_WINDOW.start_ms + FIXED_WINDOW.end_ms) / 2);
        const r = await injectViaApi(ctx.base, {
          identity: BLOCKED_IDENTITY,
          timestamp,
          text: '排 通行证 结城理 1',
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
        const first = await injectViaApi(ctx.base, {
          identity: BLOCKED_IDENTITY,
          text: '排 通行证 结城理 1',
        });
        assertEq(first.outcome.status, 'Applied', '非预存先排状态');
        assertEq(firstSlotUser(first.board), BLOCKED_IDENTITY, '非预存首格');
        const second = await injectViaApi(ctx.base, {
          identity: TARGET_IDENTITY,
          text: '排 通行证 结城理 1',
        });
        assertEq(second.outcome.status, 'Applied', '预存排状态');
        assertEq(firstSlotUser(second.board), TARGET_IDENTITY, '预存应占首格');
      },
    },
    {
      name: '批量注入 /api/sim/messages',
      fn: async (page, ctx) => {
        const now = Date.now();
        // 裸数组：1 合法 + 1 缺 timestamp + 1 非测试群。
        const bare = await injectMessages(ctx.base, [
          { user_id: TARGET_IDENTITY, nickname: TARGET_IDENTITY, text: '排 通行证 结城理 1', timestamp: now },
          { user_id: BLOCKED_IDENTITY, nickname: BLOCKED_IDENTITY, text: '缺时间戳' },
          { user_id: BLOCKED_IDENTITY, nickname: BLOCKED_IDENTITY, text: '排 通行证 岳羽由加莉 1', timestamp: now, group_id: '999999' },
        ]);
        assertEq(bare.count, 3, 'count');
        assertEq(bare.summary.applied, 1, 'applied');
        assertEq(bare.summary.dropped, 2, 'dropped');
        assertEq(bare.results[1].detail, 'missing_timestamp', '缺字段原因');
        assertEq(bare.results[2].detail, 'not_test_group', '非测试群原因');

        // messages 包装：另一条合法（不同子项）。
        const wrapped = await injectMessages(ctx.base, {
          messages: [
            { id: BLOCKED_IDENTITY, nickname: BLOCKED_IDENTITY, text: '排 通行证 虎狼丸 1', timestamp: now },
          ],
        });
        assertEq(wrapped.count, 1, 'wrapped count');
        assertEq(wrapped.summary.applied, 1, 'wrapped applied');
        assertEq(subitemFirstSlotUser(wrapped.board, 'v_hlw'), BLOCKED_IDENTITY, 'wrapped 首格');
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
        assertEq(subitemFirstSlotUser(m1.board, 'v_jcl'), TARGET_IDENTITY, 'run 回放首格');
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

        await resetViaHttp(ctx.base);

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
      name: '静态资源 200 与 /sim 已移除',
      fn: async (page, ctx) => {
        for (const urlPath of ['/', '/common.js', '/display.css', '/round.html', '/settings.html']) {
          const res = await fetch(ctx.base + urlPath, { signal: AbortSignal.timeout(10000) });
          assertEq(res.status, 200, urlPath + ' 状态');
        }
        const gone = await fetch(ctx.base + '/sim', { signal: AbortSignal.timeout(10000) });
        assertEq(gone.status, 404, '/sim 应已移除');
        const goneJs = await fetch(ctx.base + '/sim.js', { signal: AbortSignal.timeout(10000) });
        assertEq(goneJs.status, 404, '/sim.js 应已移除');
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
      name: '/api/workflow 工作流快照',
      fn: async (page, ctx) => {
        const res = await fetch(ctx.base + '/api/workflow', {
          signal: AbortSignal.timeout(10000),
        }).then((r) => r.json());
        assertEq(res.round_id, '月行水上', '轮次');
        assert('locked' in res, '应含 locked 字段');
        assert('phase' in res, '应含 phase 字段');
        assert(typeof res.version === 'number', 'version 应为数字');
        assert(typeof res.gateway === 'object' && res.gateway !== null, '应含 gateway 快照');
        assertEq(res.reply_enabled, false, 'reply_enabled 默认 false');
      },
    },
    {
      name: '消息稳定身份、原位确认与并发守卫',
      fn: verifyMessageIdentity,
    },
    {
      name: '原自动结果保留、优先名单真实保存与零模型重算',
      fn: verifyAutomaticBaseline,
    },
    {
      name: '完整动作手工编辑、忽略拒绝恢复与草稿冲突',
      fn: verifyMessageActions,
    },
    {
      name: '补充原子事务、自动组转换、单步撤回与过期代填',
      fn: verifyMessageTransactions,
    },
    {
      name: '当前轮次复盘只读与真实步骤',
      fn: async (page, ctx) => {
        await injectViaApi(ctx.base, { identity: TARGET_IDENTITY, text: '排 通行证 结城理 1' });
        await injectViaApi(ctx.base, { identity: BLOCKED_IDENTITY, text: '闲聊复盘检查' });
        const before = await api(ctx.base, 'GET', '/api/board');
        const view = await api(ctx.base, 'GET', '/api/replay/view');
        const after = await api(ctx.base, 'GET', '/api/board');
        assertEq(view.source, 'active_round', '复盘来源');
        assertEq(view.steps.length, 2, '包含已记录和忽略消息');
        assertEq(JSON.stringify(before.board), JSON.stringify(after.board), '读取不能改实时排位');
        assertEq(JSON.stringify(view.steps.at(-1).board.item_allocations), JSON.stringify(before.board.item_allocations), '末步应与实时排位一致');
        await page.goto(`${ctx.base}/replay.html?api=${encodeURIComponent(ctx.base)}`);
        await page.waitForFunction(() => document.querySelector('#step-info')?.textContent.includes('当前第 2 步'));
        assert((await page.textContent('#source-info')).includes('当前激活轮次'), '不应显示示例文件');
        await page.click('#btn-first');
        assert((await page.textContent('#step-info')).includes('当前第 1 步'), '首步切换');
        assert((await page.textContent('#who-whats-body')).includes(TARGET_IDENTITY), '当前步成员信息');
        await page.click('#tab-column');
        const readable = await page.locator('.col-table th').first().evaluate(el => getComputedStyle(el).color !== 'rgb(255, 255, 255)');
        assert(readable, '浅色表头不能为白字');
        const selectedStep = await page.textContent('#step-info');
        const diagnostic = await api(ctx.base, 'GET', '/api/replay/diff');
        assertEq(diagnostic.round_id, view.round_id, '诊断明确当前轮次，不借旧预览');
        assertEq(diagnostic.revision, view.revision, '诊断配置上下文');
        assertEq(diagnostic.comparison, 'saved_actions_vs_live_allocation', '诊断比较范围明确');
        assertEq(diagnostic.live_version, before.version, '诊断同切面当前实时版本');
        const preview = await api(ctx.base, 'POST', '/api/replay', {overrides:{items:[]}});
        assert(preview.diff.changed, '显式空目录预览确实与默认不同');
        const current = await api(ctx.base, 'GET', '/api/replay/diff');
        assertEq(current.diff.changed, false, 'GET不能继承覆盖预览');
        assertEq(JSON.stringify((await api(ctx.base, 'GET', '/api/board')).board), JSON.stringify(before.board), '预览/诊断不能暗中应用或重载排表');
        assertEq(await page.textContent('#step-info'), selectedStep, '诊断/预览不改变用户当前复盘步');
        await page.route('**/api/replay/view', route => route.fulfill({status:404,body:'接口尚未启用'}));
        await page.click('#active-refresh');
        await page.waitForFunction(() => document.querySelector('#step-info')?.textContent.includes('逐步历史尚未启用'));
        assert((await page.textContent('#replay-title')).includes(view.title), '旧服务也应显示实际激活轮次');
        assert(await page.locator('#replay-notice').isVisible(), '快照模式提示应始终可见');
        assert((await page.textContent('#table-view')).includes(TARGET_IDENTITY), '旧服务降级不能换成示例');
        await page.unroute('**/api/replay/view');
        await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
      },
    },
    {
      name: 'POST /api/rounds/:id/setup 开团前填写',
      fn: async (page, ctx) => {
        const roundId = 'e2e-setup';
        const items = [
          {
            item_id: 'pass_sp',
            name: '通行认证SP-月行水上',
            kind: '拼团',
            aliases: ['通行证'],
            unit_price_cents: 5000,
            subitems: [
              {
                subitem_id: 'v_jcl',
                name: '结城理',
                unit_price_cents: 5000,
                adjust_cents: -500,
                capacity: null,
                aliases: [],
              },
            ],
          },
          {
            item_id: 'item_1',
            name: '纪念徽章',
            kind: '单领',
            unit_price_cents: 1800,
            max_quantity: 2,
            aliases: [],
            subitems: [],
          },
        ];

        const dry = await api(ctx.base, 'POST', `/api/rounds/${roundId}/setup`, {
          dry_run: true,
          group_id: GROUP_ID,
          category: 'test',
          items,
        });
        assertEq(dry.dry_run, true, 'dry_run 标记');
        assertEq(dry.written, false, 'dry_run 不应落盘');
        assertEq(dry.ok, true, '模板应无 error 级 issue: ' + JSON.stringify(dry.issues));

        const saved = await api(ctx.base, 'POST', `/api/rounds/${roundId}/setup`, {
          group_id: GROUP_ID,
          category: 'test',
          priority_users: ['user_a'],
          items,
        });
        assertEq(saved.written, true, '应落盘');
        assertEq(saved.ok, true, '应无 error 级 issue: ' + JSON.stringify(saved.issues));
        assertEq(saved.applied_to_active, false, '非激活轮次不应热载');
        assertEq(saved.round.category, 'test', '轮次类别应回显');
        assertEq(saved.round.items, items.length, '轮次商品数');

        const list = await api(ctx.base, 'GET', '/api/rounds');
        assert(
          list.rounds.some((r) => r.round_id === roundId),
          '新轮次应出现在 /api/rounds 列表'
        );

        const mismatch = await fetch(ctx.base + `/api/rounds/${roundId}/setup`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ round_id: 'other', items: [] }),
          signal: AbortSignal.timeout(10000),
        });
        assertEq(mismatch.status, 400, 'round_id 不一致应 400');
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
      name: '桌面布局（1920×1080 / 100%）与元金额联动保存',
      fn: async (page, ctx) => {
        await injectViaApi(ctx.base, { identity: TARGET_IDENTITY, text: '排 通行证 结城理 1' });
        const original = await api(ctx.base, 'GET', '/api/config');
        try {
          await page.goto(`${ctx.base}/round.html?api=${encodeURIComponent(ctx.base)}`);
          await page.waitForSelector('.subitem-price-base');
          const rows = page.locator('.vtable tbody tr');
          for (const adjust of await page.locator('.subitem-price-adjust').all()) await adjust.fill('0.00');
          await rows.nth(0).locator('.subitem-price-base').fill('12.34');
          await rows.nth(0).locator('.subitem-price-adjust').fill('1.25');
          await rows.nth(1).locator('.subitem-price-adjust').fill('-1.25');
          assertEq(await rows.nth(0).locator('.subitem-price-final').inputValue(), '13.59', '元金额联动');
          await rows.nth(0).locator('.subitem-price-final').fill('13.84');
          await rows.nth(1).locator('.subitem-price-adjust').fill('-1.50');
          assertEq(await rows.nth(0).locator('.subitem-price-base').inputValue(), '12.34', '改最终价不能改原价');
          assertEq(await rows.nth(0).locator('.subitem-price-adjust').inputValue(), '+1.50', '反算调价');
          await page.click('#save');
          await page.waitForFunction(() => !document.querySelector('#save').disabled);
          const saved = await api(ctx.base, 'GET', '/api/config');
          assertEq(saved.config.round.items[0].subitems[0].unit_price_cents, 1234, '前端元应按分保存');
          assertEq(saved.config.round.items[0].subitems[0].adjust_cents, 150, '调价应按分保存');
          await rows.nth(0).locator('.alias-input').fill('别名甲/别名乙');
          await rows.nth(0).locator('[data-act="split"]').click();
          assertEq(await rows.nth(0).locator('.alias-input').inputValue(), '别名甲, 别名乙', '切分按钮应生效');
          assert(await page.evaluate(() => document.querySelector('#sec-phases').closest('#sec-rounds') !== null), '阶段应在01轮次管理内');
          for (const route of ['round','settings','settlement','replay','index','admin','display']) {
            await page.goto(`${ctx.base}/${route}.html?api=${encodeURIComponent(ctx.base)}&ws=${encodeURIComponent(ctx.wsUrl)}`);
            await page.waitForFunction(() => document.querySelector('#wf-api')?.textContent.includes('正常'));
            if (route === 'replay') await page.waitForFunction(() => document.querySelector('#source-info')?.textContent.includes('当前激活轮次'));
            if (route === 'settlement') {
              const view=await api(ctx.base,'GET','/api/settlement/draft');
              const cfg=(await api(ctx.base,'GET','/api/config')).config;
              const item=cfg.round.items[0],variant=item.subitems[0];
              await api(ctx.base,'PUT','/api/settlement/draft',{round_id:view.round_id,revision:view.revision,source_token:view.source_token,order_table:{packages:[{package_id:'占位包',lines:[{item_id:item.item_id,subitem_id:variant.subitem_id,qty:1,unit_price_cents:1234,is_gift:false}]}]},order_numbers:{'占位包':'ORDER-01'}});
              await page.reload();
              await page.waitForSelector('#order-table .pkg');
              await page.waitForFunction(() => document.querySelector('#draft-status')?.textContent.includes('已保存'));
            }
            if (route === 'display') await page.waitForFunction(() => document.querySelector('#conn-badge')?.textContent.includes('正常'));
            const dimensions = await page.evaluate(() => ({width:innerWidth, height:innerHeight,
              dpr:window.devicePixelRatio,
              page:document.querySelector('.page').getBoundingClientRect().width,
              header:document.querySelector('.app-topbar').getBoundingClientRect().height,
              overflow:document.documentElement.scrollWidth>innerWidth}));
            assertEq(dimensions.width,1920,'桌面画布宽');
            assertEq(dimensions.height,1080,'桌面画布高');
            assertEq(dimensions.dpr,1,route+' 应为 100% 缩放');
            assert(dimensions.page>1700, route+' 应使用宽工作区');
            assert(dimensions.header<180, route+' 头部不应挤成多行');
            assert(!dimensions.overflow, route+' 不应页面级横向溢出');
            if (process.env.PAIGU_UI_CAPTURE_DIR) {
              fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});
              await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,route+'.png')});
            }
          }
          await page.goto(`${ctx.base}/replay.html?api=${encodeURIComponent(ctx.base)}`);
          await page.waitForFunction(() => document.querySelector('#source-info')?.textContent.includes('当前激活轮次'));
          const oldRound = original.config.round.round_id;
          await api(ctx.base,'POST','/api/rounds/e2e-setup/activate',{mode:'fresh'});
          await page.waitForFunction(() => document.querySelector('#replay-notice')?.textContent.includes('暂无复盘消息'),{timeout:15000});
          assertEq((await page.textContent('#replay-title')).trim(),'e2e-setup','切换后应显示新轮次');
          assertEq(await page.locator('#step-list .step-item').count(),0,'新空轮次不能残留旧步骤');
          await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(oldRound)}/activate`,{mode:'continue'});
        } finally {
          const current = await api(ctx.base,'GET','/api/config');
          await api(ctx.base,'PUT','/api/config',{config:original.config,revision:current.revision});
          await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
        }
      },
    },
    { name: '固定宽度、盒列对齐与复盘切步', fn: verifyBoardLayout },
    { name: 'CN作用域保存、整盒关联、等效移动与锁定、私有备份预检', fn: verifyCorrectnessGates },
    { name: '七工作区42对点击、旧入口、连接上下文与只读消息真源', fn: verifyWorkspaceNavigation },
    { name: '系统四分区、作用域冲突、三选草稿与实例维护取消', fn: verifyScopedSettings },
    { name: '权限设置携草稿返回、筛选定位、复盘到处理与版本冲突', fn: verifyPolicyReturn },
    { name: '成员请求上下文、只读建议预览、迟到保护与断连接收器', fn: verifyMemberBoundaries },
    { name: '成员每日计划、失败恢复、忙碌状态与草稿保护', fn: verifyMemberPullHealth },
    { name: '具名与群外代排归属、原发言溯源和资格说明', fn: verifyProxyAttribution },
    { name: '管理员目标改单、原命令追溯、键盘返回与开关历史边界', fn: verifyTargetModify },
    { name: '私有备份原字节下载、文件预检应用、取消与迟到保护', fn: verifyPrivateBackup },
    { name: '结算来源复核、保留订单、问题定位、独立规则与并发守卫', fn: verifySettlementReview },
    { name: '关系图商品聚焦分组、完整提示、键盘定位与异步视图保留', fn: verifyReplayGraph },
    { name: '代填取消、乱序迟到、记录冲突、超时与锁定只读', fn: verifyDraftConcurrency },
    { name: '目录明暗分类、筛选与长表首中末实际点击', fn: verifyCatalogReachability },
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
  const cases = buildCases().filter(testCase=>!process.env.PAIGU_E2E_FILTER || testCase.name.includes(process.env.PAIGU_E2E_FILTER));
  if(!cases.length)throw new Error('PAIGU_E2E_FILTER did not match any case');

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
          subitem_id: 'v_jcl',
          boxes: [
            { box_index: 1, slots: [{ slot_index: 1, user_id: '远程用户', status: 'filled' }] },
          ],
        },
      ],
    });
    const ctx = { base, remote, wsUrl, tmpDir };

    browser = await chromium.launch();
    // 桌面验收：1920×1080、100%（deviceScaleFactor = 1）。
    const context = await browser.newContext({
      viewport: { width: 1920, height: 1080 },
      deviceScaleFactor: 1,
    });
    const page = await context.newPage();
    page.setDefaultTimeout(15000);

    await page.goto(`${base}/display.html?api=${encodeURIComponent(base)}`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(() => typeof window.PAIGU !== 'undefined');

    for (const testCase of cases) {
      if (testCase.name !== 'remote 数据源') {
        await resetViaHttp(base);
      }
      try {
        await testCase.fn(page, ctx);
        log(`PASS ${testCase.name}`);
      } catch (err) {
        failures += 1;
        log(`FAIL ${testCase.name} ${err && err.stack ? err.stack : err}`);
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
