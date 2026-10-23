import fs from 'node:fs';
import path from 'node:path';

function check(value, message) { if (!value) throw new Error(message); }
async function api(base, method, route, body) {
  const response = await fetch(base + route, {method, headers: {'Content-Type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(20000)});
  const value = await response.json(); check(response.ok, route + ': ' + response.status); return value;
}
const count = board => board.user_summaries.flatMap(row => row.items).reduce((n, item) => n + item.quantity, 0);

export async function verifyIncrementalFresh(page, ctx) {
  const initial = await api(ctx.base, 'GET', '/api/config'), round = 'e2e-incremental-fresh';
  const errors = [], onError = error => errors.push(String(error)); page.on('pageerror', onError);
  try {
    check(!initial.config.llm.enabled && !initial.config.gateway.reply_enabled, 'isolated receiver/model');
    await api(ctx.base, 'POST', '/api/rounds', {round_id: round, title: '尾组与清空验收', copy_from: initial.config.round.round_id, category: 'test'});
    await api(ctx.base, 'POST', `/api/rounds/${round}/setup`, {phases: [], priority_users: [], priority_window: null,
      items: [{item_id: 'badge', name: '徽章', kind: '拼团', aliases: [], unit_price_cents: 1000,
        subitems: [{subitem_id: 'a', name: '甲', aliases: [], capacity: 1, unit_price_cents: 1000, adjust_cents: 0}]}]});
    await api(ctx.base, 'POST', `/api/rounds/${round}/activate`, {mode: 'fresh'});
    const config = await api(ctx.base, 'GET', '/api/config');
    config.config.merge.enabled = true; config.config.merge.same_sender = true; config.config.gateway.whitelist_members = [];
    await api(ctx.base, 'PUT', '/api/config', {config: config.config, revision: config.revision});
    const send = (text, user, timestamp) => api(ctx.base, 'POST', '/api/sim/messages', [{
      user_id: user, nickname: user, group_id: config.config.round.group_id, text, timestamp,
    }]);
    for (let index = 0; index < 4; index++) await send('排 徽章 甲 1', '成员' + Math.min(index, 2), 1000 + index);
    check(count((await api(ctx.base, 'GET', '/api/board')).board) === 4, 'real frozen tail grows without duplicating earlier groups');
    const checkpoint = path.join(ctx.tmpDir, 'config', 'computed', round + '.json');
    check(fs.existsSync(checkpoint), 'complete merge projection has an actual private checkpoint');
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(() => document.querySelectorAll('#messages .msg').length === 4);
    await api(ctx.base, 'POST', `/api/rounds/${round}/activate`, {mode: 'fresh'});
    await send('排 徽章 甲 2', '成员清空后', 2000);
    await page.locator('#refresh').click();
    await page.waitForFunction(() => document.querySelectorAll('#messages .msg').length === 1);
    check((await page.locator('#messages').textContent()).includes('成员清空后'), 'fresh removes stale rows in the real open stream');
    check(count((await api(ctx.base, 'GET', '/api/board')).board) === 2, 'fresh does not revive four historical units');
    check(!fs.existsSync(checkpoint), 'partial fresh projection cannot be saved as the complete source');
    await page.goto(`${ctx.base}/replay.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(() => document.querySelector('#step-info')?.textContent.includes('当前第 1 步'));
    const selected = await page.locator('#step-info').textContent(); await page.locator('#tab-column').click();
    check(await page.locator('#step-info').textContent() === selected, 'view switch preserves the original single message step');
    const live = (await api(ctx.base, 'GET', '/api/board')).board;
    const preview = await api(ctx.base, 'POST', '/api/replay', {});
    check(preview.messages.length === 5 && count(preview.board) === 6, 'explicit complete-source preview retains all original messages');
    check(JSON.stringify((await api(ctx.base, 'GET', '/api/board')).board) === JSON.stringify(live), 'preview cannot remove the temporary live cut');
    await api(ctx.base, 'POST', '/api/sim/reset', {});
    await send('排 徽章 甲 1', '成员重置后', 500);
    check(count((await api(ctx.base, 'GET', '/api/board')).board) === 1, 'HTTP reset uses identity cut, not a timestamp cutoff');
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(() => document.querySelectorAll('#messages .msg').length === 1);
    check((await page.locator('#messages').textContent()).includes('成员重置后'), 'reset does not retain stale old DOM rows');
    check(fs.readFileSync(path.join(ctx.tmpDir, 'config', 'messages', round + '.jsonl'), 'utf8').trim().split('\n').length === 6, 'neither fresh nor HTTP reset deletes source rows');
    check(await page.evaluate(() => innerWidth === 1920 && innerHeight === 1080 && devicePixelRatio === 1 && document.documentElement.scrollWidth <= innerWidth + 1), 'desktop 100% without horizontal page overflow');
    check(!errors.length, errors.join(';'));
  } finally {
    page.off('pageerror', onError);
    await api(ctx.base, 'POST', `/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`, {mode: 'fresh'});
    const current = await api(ctx.base, 'GET', '/api/config');
    current.config.merge = initial.config.merge; current.config.gateway.whitelist_members = initial.config.gateway.whitelist_members;
    await api(ctx.base, 'PUT', '/api/config', {config: current.config, revision: current.revision});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
