import fs from 'node:fs';
import path from 'node:path';

function check(condition, message) { if (!condition) throw new Error(message); }
async function request(base, method, route, body, status = 200) {
  const res = await fetch(base + route, {
    method, headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(10000),
  });
  const data = await res.json();
  check(res.status === status, `${method} ${route}: ${res.status}, expected ${status}`);
  return data;
}

/** Real isolated API + real browser interaction, not an overridden display response. */
export async function verifyMessageIdentity(page, ctx) {
  const errors = [];
  const onError = error => errors.push(String(error));
  page.on('pageerror', onError);
  const originalCfg = (await request(ctx.base, 'GET', '/api/config')).config;
  check(!originalCfg.llm.enabled && !originalCfg.gateway.reply_enabled, 'test must not call a real model or group');
  // reset clears live memory, not historical JSONL. Use a fresh isolated round so
  // recompute cannot accidentally bring back earlier cases and mask stale-row bugs.
  const roundId = 'e2e-message-identity';
  try {
    await request(ctx.base, 'POST', '/api/rounds', { round_id: roundId, title: '消息身份验收', copy_from: originalCfg.round.round_id, category: 'test' });
    await request(ctx.base, 'POST', `/api/rounds/${roundId}/activate`, { mode: 'fresh' });
    const cfg = (await request(ctx.base, 'GET', '/api/config')).config;
    const send = (user, text, time) => request(ctx.base, 'POST', '/api/sim/messages', [{
      user_id: user, nickname: user, text, timestamp: time, group_id: cfg.round.group_id,
    }]);
    // Ambiguous input opens the same action editor used by all other local statuses.
    const text = '排 通行证 结城理 1 撤';
    const now = Date.now();
    await send('成员03', text, now);
    await send('成员04', '排 通行证 虎狼丸 1', now + 1000);
    await request(ctx.base, 'POST', '/api/sim/messages', Array.from({ length: 35 }, (_, i) => ({
      user_id: '成员05', nickname: '成员05', text: '占位闲聊 ' + i,
      timestamp: now + 2000 + i, group_id: cfg.round.group_id,
    })));
    const before = await request(ctx.base, 'GET', '/api/messages?since=0');
    const target = before.messages.find(m => m.text === text);
    const neighbour = before.messages.find(m => m.user_id === '成员04');
    check(target?.status === 'NeedConfirm', 'fixture must actually require confirmation');
    check(target.record_id && neighbour.record_id, 'persistent stable ids required');
    const original = structuredClone(target);
    const endpoint = '/api/messages/' + encodeURIComponent(target.record_id) + '/actions';
    let lastBody;
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(id => [...document.querySelectorAll('#messages .msg')].some(n => n.__msg?.record_id === id), target.record_id);
    const node = page.locator('#messages .msg.needconfirm').filter({ hasText: text });
    const handle = await node.elementHandle();
    await node.focus();
    const scrollBefore = await page.locator('#messages').evaluate(el => {
      el.scrollTop = 0;
      if (el.scrollHeight <= el.clientHeight) throw new Error('fixture must exercise an actual scrolling message list');
      return el.scrollTop;
    });
    check(!(await page.locator('#new-content').isVisible()), 'hidden unread control must not render as an empty blue rectangle');
    await page.keyboard.press('Enter');
    const dialog = page.getByRole('dialog', { name: '修订消息动作' });
    await dialog.waitFor();
    await dialog.locator('.ae-treatment').waitFor();
    check(await dialog.locator('.ae-treatment').evaluate(el => el === document.activeElement), 'empty draft starts at treatment field');
    await dialog.locator('.ov-ok').focus();
    await page.keyboard.press('Tab');
    check(await dialog.locator('.ov-cancel').evaluate(el => el === document.activeElement), 'Tab must stay inside dialog');
    await page.keyboard.press('Escape');
    check(await node.evaluate(el => el === document.activeElement), 'closing dialog restores focus to the same message');
    await node.click();
    await dialog.locator('.ae-add-action').click();
    await dialog.locator('.ov-item').fill('通行证');
    await dialog.locator('.ov-subitem').fill('结城理');
    await dialog.locator('.ov-qty').fill('0');
    await dialog.locator('.ov-ok').click();
    check((await dialog.locator('.modal-err').textContent()).includes('1–99'), 'invalid quantity must be explained, not silently changed to 1');
    await dialog.locator('.ov-qty').fill('2');
    check((await dialog.locator('.modal-err').textContent()) === '', 'editing clears the obsolete validation error');
    const capture = async name => {
      if (!process.env.PAIGU_UI_CAPTURE_DIR) return;
      fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR, { recursive: true });
      await page.screenshot({ path: path.join(process.env.PAIGU_UI_CAPTURE_DIR, name + '.png') });
    };
    await capture('message-identity-dialog');
    const sent = page.waitForRequest(req => req.method() === 'POST' && new URL(req.url()).pathname === endpoint);
    const received = page.waitForResponse(res => res.request().method() === 'POST' && new URL(res.url()).pathname === endpoint);
    await dialog.locator('.ov-ok').click();
    lastBody = (await sent).postDataJSON();
    check((await received).ok(), 'one save must succeed');
    check(lastBody.record_revision === target.record_revision && lastBody.round_id === cfg.round.round_id, 'save must bind the opened revision and round');
    await dialog.waitFor({ state: 'detached' });
    check(await handle.evaluate(el => el.isConnected && el.__msg.record_revision === 2), 'same DOM record updated in place');
    check(Math.abs((await page.locator('#messages').evaluate(el => el.scrollTop)) - scrollBefore) <= 1, 'updating an old row must not scroll to the bottom');
    const after = (await request(ctx.base, 'GET', '/api/messages?since=0')).messages;
    const changed = after.find(m => m.record_id === target.record_id);
    for (const key of ['record_id', 'seq', 'text', 'timestamp_ms', 'user_id', 'message_id']) {
      check(changed[key] === original[key], `${key} must stay unchanged`);
    }
    check(changed.record_revision === 2 && changed.status === 'Applied', 'successful save updates revision/status');
    for (const key of ['record_id','record_revision','seq','text','timestamp_ms','status']) check(after.find(m => m.record_id === neighbour.record_id)[key] === neighbour[key], 'neighbour must not be modified: ' + key);
    check(after.length === before.messages.length, 'editing does not append a new member message');
    const duplicate = await request(ctx.base, 'POST', endpoint, lastBody);
    check(duplicate.deduplicated, 'same transaction retry must not create another edit');
    const stale = await request(ctx.base, 'POST', endpoint, { ...lastBody, transaction_id: 'fresh-stale-request' }, 409);
    check(stale.error === 'stale_record_revision', 'stale editor receives useful conflict');
    const otherRound = await request(ctx.base, 'POST', endpoint, { ...lastBody, record_revision: 2, round_id: 'other_round' }, 409);
    check(otherRound.error === 'round_changed', 'do not write to a different active round');
    await request(ctx.base, 'POST', `/api/messages/${target.seq}/actions`, { ...lastBody, record_revision: 2 }, 404);
    await capture('message-identity-applied');
    check(!errors.length, 'browser errors: ' + errors.join('; '));
  } finally {
    page.off('pageerror', onError);
    await request(ctx.base, 'POST', `/api/rounds/${encodeURIComponent(originalCfg.round.round_id)}/activate`, { mode: 'fresh' });
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
