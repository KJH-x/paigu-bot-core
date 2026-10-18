import fs from 'node:fs';
import path from 'node:path';

function check(v, m) { if (!v) throw new Error(m); }
async function api(base, method, route, body, status = 200) {
  const r = await fetch(base + route, { method, headers: { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(15000) });
  const data = await r.json(); check(r.status === status, `${method} ${route}: ${r.status}, expected ${status}`); return data;
}
export async function verifyMessageActions(page, ctx) {
  const initial = await api(ctx.base, 'GET', '/api/config'), round = 'e2e-actions';
  const errors = [], onError = e => errors.push(String(e)); page.on('pageerror', onError);
  const capture = async name => { if (process.env.PAIGU_UI_CAPTURE_DIR) { fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR, { recursive: true }); await page.screenshot({ path: path.join(process.env.PAIGU_UI_CAPTURE_DIR, name + '.png') }); } };
  try {
    check(!initial.config.llm.enabled && !initial.config.gateway.reply_enabled, 'no real model or group actions');
    await api(ctx.base, 'POST', '/api/rounds', { round_id: round, title: '消息动作验收', category: 'test', copy_from: initial.config.round.round_id });
    await api(ctx.base, 'POST', `/api/rounds/${round}/setup`, { phases: [], priority_users: [], priority_window: null,
      items: [{ item_id: 'badge', name: '徽章', kind: '拼团', aliases: [], unit_price_cents: 1000,
        subitems: [{ subitem_id: 'a', name: '甲', aliases: [], capacity: 1, unit_price_cents: 1000, adjust_cents: 0 }] }] });
    await api(ctx.base, 'POST', `/api/rounds/${round}/activate`, { mode: 'fresh' });
    const now = Date.now(), send = text => api(ctx.base, 'POST', '/api/sim/messages', [{ user_id: '成员01', nickname: '成员01', group_id: initial.config.round.group_id, text, timestamp: now }]);
    await send('排 徽章 甲 1');
    const record = (await api(ctx.base, 'GET', '/api/messages?since=0')).messages[0];
    const route = `/api/messages/${record.record_id}/actions`, read = () => api(ctx.base, 'GET', route + '?round_id=' + round);
    const before = await read(), baseline = JSON.stringify(before.original_automatic);
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    const row = page.locator('#messages .msg').filter({ hasText: record.text }), dialog = page.getByRole('dialog', { name: '修订消息动作' });
    await row.waitFor(); await row.click(); await dialog.locator('.ov-qty').waitFor();
    check(await dialog.locator('.ov-qty').inputValue() === '1', 'existing action quantity is prefilled');
    check(await dialog.locator('.ov-item').inputValue() === '徽章', 'existing canonical product is prefilled with name');
    check(await dialog.locator('.ov-subitem').inputValue() === '甲', 'existing subitem is preserved');
    await dialog.locator('.ae-compare summary').click();
    await dialog.locator('.ov-qty').fill('3'); await dialog.locator('.ae-note').fill('管理员说明：当时指三份');
    await capture('message-actions-edit-light');
    const save = async () => {
      const response = page.waitForResponse(r => r.request().method() === 'POST' && new URL(r.url()).pathname === route);
      await dialog.locator('.ov-ok').click(); const r = await response; check(r.ok(), 'one save succeeds'); await dialog.waitFor({ state: 'detached' }); return r.json();
    };
    await save();
    check((await read()).editable_selection.actions[0].items[0].quantity === 3, 'quantity edit saved');
    await page.waitForFunction(id => [...document.querySelectorAll('#messages .msg')].some(n => n.__msg?.record_id === id && n.textContent.includes('当时指三份')), record.record_id);
    for (const [mode, expected] of [['ignore','Ignored'],['reject','Rejected'],['automatic','Applied']]) {
      await row.click(); await dialog.locator('.ae-treatment').selectOption(mode);
      await save(); const current = await read();
      check(current.current_outcome.status === expected, 'saved disposition: ' + mode);
      check(JSON.stringify(current.original_automatic) === baseline, 'automatic baseline never overwritten: ' + mode);
    }
    await row.click(); await dialog.locator('.ov-qty').waitFor();
    check(await dialog.locator('.ov-qty').inputValue() === '1', 'restore automatic brings original quantity back');
    await dialog.locator('.ae-add-action').click();
    await dialog.locator('.ov-item').nth(1).fill('徽章'); await dialog.locator('.ov-subitem').nth(1).fill('甲'); await dialog.locator('.ov-qty').nth(1).fill('2');
    await dialog.locator('.ae-action-head select').nth(1).selectOption('modify');
    await dialog.locator('.ov-qty').nth(1).fill('2');
    await capture('message-actions-multiple');
    await save();
    const selected = (await read()).editable_selection;
    check(selected.actions.length === 2 && selected.actions[1].operation === 'modify', 'ordered multiple actions saved');
    const board = await api(ctx.base, 'GET', '/api/board');
    const snapshot = board.board || board;
    check(snapshot.user_summaries[0].items[0].quantity === 2, 'modify cancels earlier same-message claim before replacing');
    const initialTheme = await page.locator('html').getAttribute('data-theme');
    if (initialTheme !== 'dark') await page.locator('#theme-toggle').click();
    await row.click(); await dialog.locator('.ov-qty').first().waitFor();
    await dialog.locator('.ae-item-options summary').first().click();
    check(await dialog.getByRole('combobox', { name: '位置策略', exact: true }).isVisible(), 'advanced strategy is reachable');
    await capture('message-actions-edit-dark');
    for (let i = 0; i < 4; i++) await dialog.locator('.ae-add-action').click();
    const scroll = await dialog.locator('.ae-content').evaluate(n => { n.scrollTop = n.scrollHeight; return { top: n.scrollTop, overflow: n.scrollHeight > n.clientHeight }; });
    check(scroll.overflow && scroll.top > 0, 'long editor has usable inner scrolling');
    check(await dialog.locator('.ov-ok').isVisible(), 'save remains reachable while body scrolls');
    check(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'editor does not overflow page width');
    await capture('message-actions-scroll-dark');
    await page.keyboard.press('Escape');
    await page.locator('.draft-guard [data-choice="cancel"]').click();
    check(await dialog.locator('.ae-action').count()>=4,'cancel leaving keeps the complete multi-action draft');
    await page.keyboard.press('Escape');
    await page.locator('.draft-guard [data-choice="discard"]').click();
    if (initialTheme !== 'dark') await page.locator('#theme-toggle').click();
    // A stale, still-open draft must not overwrite another successful human save.
    await row.click(); await dialog.locator('.ov-qty').first().fill('8');
    const concurrent = await read();
    await api(ctx.base, 'POST', route, { transaction_id: 'concurrent-human', round_id: round, config_revision: concurrent.config_revision, record_revision: concurrent.message.record_revision, selection: { kind: 'reject', note: '另一个管理操作' } });
    const conflict = page.waitForResponse(r => r.request().method() === 'POST' && new URL(r.url()).pathname === route);
    await dialog.locator('.ov-ok').click(); check((await conflict).status() === 409, 'stale draft returns 409');
    check(await dialog.locator('.ov-qty').first().inputValue() === '8', 'conflict keeps edited draft');
    await dialog.locator('.modal-err').filter({ hasText: '草稿保留' }).waitFor();
    await capture('message-actions-conflict');
    await page.keyboard.press('Escape');
    await page.locator('.draft-guard [data-choice="discard"]').click();
    check(await row.evaluate(n => n === document.activeElement), 'focus returns to original row');
    // A saved exact target can later disappear; do not silently retarget another claim.
    const revise=async(id,selection)=>{const current=await api(ctx.base,'GET',`/api/messages/${id}/actions?round_id=${round}`);return api(ctx.base,'POST',`/api/messages/${id}/actions`,{transaction_id:'final-'+Math.random().toString(36).slice(2),round_id:round,config_revision:current.config_revision,record_revision:current.message.record_revision,selection});};
    await revise(record.record_id,{kind:'automatic',note:'恢复自动作为精确撤销验收基线'});
    await send('排 徽章 甲 1');await send('撤 徽章 1');
    const raw=(await api(ctx.base,'GET','/api/messages')).messages, cancellation=raw[raw.length-1];
    const cancelRoute=`/api/messages/${cancellation.record_id}/actions`;
    const cancelDetails=await api(ctx.base,'GET',cancelRoute+'?round_id='+round);
    const exactTarget=cancelDetails.cancel_targets[0].claim_id,exact=structuredClone(cancelDetails.editable_selection);
    exact.actions[0].target_claim_id=exactTarget;exact.actions[0].target_item_id=null;exact.actions[0].quantity=null;
    await revise(cancellation.record_id,exact);
    await revise(record.record_id,{kind:'ignore',note:'第一条原认购不执行'});
    await page.locator('#refresh').click();
    const cancelledRow=page.locator('#messages .msg').filter({hasText:'撤 徽章 1'});
    await cancelledRow.click();
    await dialog.locator('.ae-original').filter({hasText:'撤销未命中'}).waitFor();
    check(await dialog.getByRole('combobox',{name:'原申购',exact:true}).inputValue()===exactTarget,'expired target remains the exact old reference');
    check((await dialog.getByRole('combobox',{name:'原申购',exact:true}).locator('option:checked').textContent()).includes('原目标已失效'),'expired target has an explicit readable label');
    check((await api(ctx.base,'GET','/api/board')).board.user_summaries[0].items[0].quantity===1,'another claim remains allocated');
    await capture('message-expired-exact-cancel');await page.keyboard.press('Escape');
    check(!errors.length, errors.join('; '));
  } finally {
    page.off('pageerror', onError);
    await api(ctx.base, 'POST', `/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`, { mode: 'fresh' });
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
