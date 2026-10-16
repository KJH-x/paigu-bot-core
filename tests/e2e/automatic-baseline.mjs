import fs from 'node:fs';
import path from 'node:path';

function check(value, message) { if (!value) throw new Error(message); }
async function api(base, method, route, body, status = 200) {
  const res = await fetch(base + route, {
    method, headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(15000),
  });
  const result = await res.json();
  check(res.status === status, `${method} ${route}: expected ${status}, got ${res.status}`);
  return result;
}

export async function verifyAutomaticBaseline(page, ctx) {
  const initial = await api(ctx.base, 'GET', '/api/config');
  const roundId = 'e2e-automatic-baseline';
  const errors = [];
  const onError = error => errors.push(String(error));
  page.on('pageerror', onError);
  try {
    check(!initial.config.llm.enabled && !initial.config.gateway.reply_enabled, 'isolation must disable real models/group replies');
    await api(ctx.base, 'POST', '/api/rounds', { round_id: roundId, copy_from: initial.config.round.round_id, title: '自动结果验收', category: 'test' });
    const now = Date.now();
    await api(ctx.base, 'POST', `/api/rounds/${roundId}/setup`, {
      priority_users: [], priority_window: { start_ms: now - 60000, end_ms: now + 600000 }, phases: [],
      items: [{ item_id: 'badge', name: '徽章', kind: '拼团', aliases: [], unit_price_cents: 1000,
        subitems: [{ subitem_id: 'a', name: '甲', aliases: [], capacity: 1, unit_price_cents: 1000, adjust_cents: 0 }] }],
    });
    await api(ctx.base, 'POST', `/api/rounds/${roundId}/activate`, { mode: 'fresh' });
    await api(ctx.base, 'POST', '/api/sim/messages', [{
      user_id: '成员03', nickname: '成员03', group_id: initial.config.round.group_id,
      text: '排 徽章 甲 2', timestamp: now,
    }]);
    const before = (await api(ctx.base, 'GET', '/api/messages?since=0')).messages;
    check(before.length === 1 && before[0].status === 'Rejected', 'non-priority claim must initially be rejected');
    const record = before[0];
    const route = `/api/messages/${encodeURIComponent(record.record_id)}/actions?round_id=${roundId}`;
    const original = await api(ctx.base, 'GET', route);
    check(original.original_automatic.original_outcome.status === 'Rejected', 'original rejection must be retained');
    check(original.original_automatic.understanding.events.length === 1, 'understood action must survive permission rejection');
    const savedBaseline = JSON.stringify(original.original_automatic);
    check(original.effective_source === 'saved_automatic' && original.automatic_matches, 'editor reads saved automatic unit');
    const snapshot = JSON.stringify((await api(ctx.base, 'GET', '/api/messages?since=0')).messages);
    await api(ctx.base, 'GET', route);
    check(snapshot === JSON.stringify((await api(ctx.base, 'GET', '/api/messages?since=0')).messages), 'opening details must not write anything');

    await page.goto(`${ctx.base}/round.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(id => document.querySelector('#rd-id')?.value === id, roundId);
    await page.locator('#rd-priority').fill('成员03');
    const saved = page.waitForResponse(res => res.request().method() === 'PUT' && new URL(res.url()).pathname === '/api/config');
    await page.locator('#save').click();
    const response = await saved;
    check(response.ok(), 'saving priority list must succeed');
    check((await response.json()).recomputed, 'priority-only save must trigger deterministic recompute');
    const current = await api(ctx.base, 'GET', route);
    check(current.current_outcome.status === 'Applied', 'saved action is reevaluated after priority edit');
    check(JSON.stringify(current.original_automatic) === savedBaseline, 'current outcome must not overwrite original automatic outcome');
    check(current.effective_source === 'saved_automatic', 'priority edit does not create a human parse override');
    await api(ctx.base, 'PUT', `/api/messages/${record.record_id}`, { record_revision: 1, text: '覆盖原文' }, 400);
    await api(ctx.base, 'DELETE', `/api/messages/${record.record_id}?record_revision=1`, undefined, 422);
    await api(ctx.base, 'GET', `/api/messages/${record.seq}/actions?round_id=${roundId}`, undefined, 404);
    await api(ctx.base, 'GET', `/api/messages/${record.record_id}/actions?round_id=other_round`, undefined, 409);
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(id => [...document.querySelectorAll('#messages .msg')].some(node => node.__msg?.record_id === id && node.__msg.status === 'Applied'), record.record_id);
    const replay = await api(ctx.base, 'GET', '/api/replay/view');
    check(replay.messages[0].original_automatic.outcome.status === 'Rejected', 'replay includes original automatic outcome');
    check(replay.messages[0].status === 'Applied', 'replay includes current permission result separately');
    if (process.env.PAIGU_UI_CAPTURE_DIR) {
      fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR, { recursive: true });
      await page.screenshot({ path: path.join(process.env.PAIGU_UI_CAPTURE_DIR, 'automatic-priority-recomputed.png') });
    }
    check(!errors.length, 'browser errors: ' + errors.join('; '));
  } finally {
    page.off('pageerror', onError);
    await api(ctx.base, 'POST', `/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`, { mode: 'fresh' });
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
