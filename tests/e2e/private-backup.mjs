import fs from 'node:fs';
import path from 'node:path';

function check(value, message) { if (!value) throw new Error(message); }
async function api(base, method, route, body) {
  const response = await fetch(base + route, {method, headers: {'Content-Type': 'application/json'},
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(15000)});
  const data = await response.json(); check(response.ok, route + ': ' + response.status); return data;
}
export async function verifyPrivateBackup(page, ctx) {
  const initial = await api(ctx.base, 'GET', '/api/config'), round = 'e2e-private-backup';
  const errors = [], requests = [], downloads = [];
  const onError = error => errors.push(String(error));
  const onRequest = request => { if (request.method() === 'POST' && request.url().includes('/api/snapshot/')) requests.push(request); };
  const onDownload = download => downloads.push(download);
  page.on('pageerror', onError); page.on('request', onRequest); page.on('download', onDownload);
  const goto = async () => {
    await page.goto(`${ctx.base}/settings.html?api=${encodeURIComponent(ctx.base)}#sec-snapshot`);
    await page.reload();
    await page.waitForFunction(() => document.querySelector('#snap-export') && !document.querySelector('#snap-export').disabled).catch(async error => {
      throw new Error(error.message + ' backup state ' + JSON.stringify(await page.evaluate(() => ({
        url: location.href, info: document.querySelector('#snap-context')?.textContent,
        banner: document.querySelector('#banner')?.textContent, workflow: window.PAIGU_SHELL?.getState(),
      }))));
    });
  };
  const preflight = async (expected = 200) => {
    const response = page.waitForResponse(r => r.request().method() === 'POST' && r.url().includes('/api/snapshot/import-file?'));
    await page.locator('#snap-import').click(); check((await response).status() === expected, 'raw file preflight status ' + expected);
    await page.waitForFunction(() => !document.querySelector('#snap-import').disabled);
  };
  const capture = async name => {
    if (!process.env.PAIGU_UI_CAPTURE_DIR) return;
    fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR, {recursive: true});
    await page.screenshot({path: path.join(process.env.PAIGU_UI_CAPTURE_DIR, name + '.png')});
  };
  try {
    check(!initial.config.llm.enabled && !initial.config.gateway.reply_enabled, 'isolated disabled model/reply');
    await api(ctx.base, 'POST', '/api/rounds', {round_id: round, title: '私有备份验收', copy_from: initial.config.round.round_id, category: 'test'});
    await api(ctx.base, 'POST', `/api/rounds/${round}/activate`, {mode: 'fresh'});
    let cfg = await api(ctx.base, 'GET', '/api/config');
    const send = text => api(ctx.base, 'POST', '/api/sim/messages', [{user_id: '成员01', nickname: '成员01', group_id: cfg.config.round.group_id, text, timestamp: Date.now()}]);
    await send('占位备份原消息');
    // Fixture-only raw source field in this owned test server's private journal.
    // The production browser must never parse/re-encode this integer as Number.
    const journal = path.join(ctx.tmpDir, 'config', 'events', round + '.jsonl');
    fs.mkdirSync(path.dirname(journal), {recursive: true});
    fs.appendFileSync(journal, '{"opaque_id":9007199254740993}\n');
    await goto();
    check(await page.locator('#snap-download').isDisabled(), 'private acknowledgment required');
    await page.locator('a[href="#sec-gateway"]').click();
    await page.locator('#gw-heartbeat').fill(String(cfg.config.gateway.heartbeat_secs + 7));
    const draftValue = await page.locator('#gw-heartbeat').inputValue();
    await page.locator('a[href="#sec-snapshot"]').click();
    await page.locator('#snap-private-ack').check();
    let response = page.waitForResponse(r => r.url().endsWith('/api/snapshot/download'));
    const event = page.waitForEvent('download');
    await page.locator('#snap-download').click();
    const downloaded = await event, headers = (await response).headers();
    check(headers['cache-control'] === 'no-store, private', 'no cache for private attachment');
    const filename = downloaded.suggestedFilename();
    check(/^paigu-private-[a-f0-9]{64}\.snapshot\.json$/.test(filename), 'opaque safe filename without member/round');
    const saved = path.join(ctx.tmpDir, filename); await downloaded.saveAs(saved);
    const raw = fs.readFileSync(saved, 'utf8');
    check(raw.includes('9007199254740993'), 'browser downloaded original exact bytes');
    check((await api(ctx.base, 'GET', '/api/config')).revision === cfg.revision, 'download does not save config draft');
    check(await page.locator('#gw-heartbeat').inputValue() === draftValue, 'download keeps unrelated draft');
    check(await page.locator('#snap-path').inputValue() === '', 'browser download never invents a server copy path');
    check(!fs.existsSync(path.join(ctx.tmpDir, 'config', 'snapshots')), 'no automatic host-path backup');
    const count = requests.length;
    await page.locator('#snap-file').setInputFiles(saved);
    await page.waitForTimeout(120);
    check(requests.length === count && await page.locator('#snap-apply').isDisabled(), 'select is not upload or apply');
    await preflight();
    check(!await page.locator('#snap-apply').isDisabled(), 'verified raw file enables explicit apply');
    const applyCount = requests.length;
    await page.locator('#snap-apply').click();
    await page.locator('.draft-guard [data-choice="cancel"]').click();
    check(requests.length === applyCount, 'cancel draft guard never applies');
    check(await page.locator('#gw-heartbeat').inputValue() === draftValue, 'canceled application retains draft');
    await send('占位预检后消息');
    response = page.waitForResponse(r => r.request().method() === 'POST' && r.url().includes('/api/snapshot/import-file?'));
    await page.locator('#snap-apply').click();
    await page.locator('.draft-guard [data-choice="discard"]').click();
    check((await response).status() === 409, 'new input invalidates raw-file proof');
    await page.waitForFunction(() => document.querySelector('#snap-apply').disabled && !document.querySelector('#snap-import').disabled);
    check((await api(ctx.base, 'GET', '/api/messages')).messages.length === 2, 'stale apply does not erase input');
    await preflight();
    for (const theme of ['light', 'dark']) {
      await page.evaluate(value => PAIGU_SHELL.theme.set(value), theme);
      await page.waitForTimeout(220); // Let the actual 120ms background transition finish.
      check(await page.locator('#snap-path').evaluate(node => {
        const color = value => value.match(/[\d.]+/g).slice(0, 3).map(Number).map(v => {v /= 255; return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4;});
        const luminance = value => {const [r, g, b] = color(value); return .2126 * r + .7152 * g + .0722 * b;};
        const style = getComputedStyle(node), a = luminance(style.color), b = luminance(style.backgroundColor);
        return (Math.max(a, b) + .05) / (Math.min(a, b) + .05) >= 4.5;
      }), 'stable theme field contrast is readable');
      await page.locator('#snap-apply').focus();
      await capture('private-backup-' + theme);
      check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1 && devicePixelRatio === 1 && innerWidth === 1920 && innerHeight === 1080), 'desktop 100% and no page overflow');
    }
    response = page.waitForResponse(r => r.request().method() === 'POST' && r.url().includes('/api/snapshot/import-file?'));
    await page.locator('#snap-apply').press('Enter'); check((await response).ok(), 'keyboard explicit apply succeeds');
    await page.waitForFunction(() => document.querySelector('#conn-badge').textContent === '已连接' && document.querySelector('#snap-apply').disabled);
    check((await api(ctx.base, 'GET', '/api/messages')).messages.length === 1, 'selected backup restores old message cut');
    check(fs.readFileSync(journal, 'utf8').includes('9007199254740993'), 'raw upload preserves exact integer in restored journal');

    await page.locator('#snap-file').setInputFiles(saved); await preflight();
    await page.locator('#snap-path').fill(saved);
    check(await page.locator('#snap-apply').isDisabled() && await page.locator('#snap-file').inputValue() === '', 'changing to path invalidates file proof');
    const bad = path.join(ctx.tmpDir, 'bad-backup.json'); fs.writeFileSync(bad, raw.replace('paigu-snapshot/1', 'wrong-format'));
    await page.locator('#snap-file').setInputFiles(bad); await preflight(400);
    check(await page.locator('#snap-path').inputValue() === '' && await page.locator('#snap-apply').isDisabled(), 'bad format clears proof and path');
    const large = path.join(ctx.tmpDir, 'oversize-backup.json');
    const fd = fs.openSync(large, 'w'); try { fs.ftruncateSync(fd, 64 * 1024 * 1024 + 1); } finally { fs.closeSync(fd); }
    const beforeLarge = requests.length; await page.locator('#snap-file').setInputFiles(large);
    check(await page.locator('#snap-import').isDisabled() && requests.length === beforeLarge, 'oversize file refuses without upload');
    await page.locator('#snap-file').setInputFiles(saved);

    // Hold real endpoint responses; cancel/withdraw/context/timeout may not
    // deliver a stale attachment even if the server completed it successfully.
    for (const reason of ['cancel', 'ack', 'context', 'api', 'timeout']) {
      await goto(); await page.locator('#snap-private-ack').check();
      let release, ready;
      const held = new Promise(resolve => { release = resolve; });
      const seen = new Promise(resolve => { ready = resolve; });
      const route = async route => { const response = await route.fetch(); ready(); await held; await route.fulfill({response}).catch(() => {}); };
      await page.route('**/api/snapshot/download', route);
      const before = downloads.length;
      if (reason === 'timeout') await page.evaluate(() => {
        window.backupTestTimeout = window.setTimeout;
        window.setTimeout = (fn, delay, ...args) => window.backupTestTimeout(fn, delay === 60000 ? 150 : delay, ...args);
      });
      try {
        await page.locator('#snap-download').click(); await seen;
        if (reason === 'cancel') await page.locator('#snap-download-cancel').click();
        if (reason === 'ack') await page.locator('#snap-private-ack').uncheck();
        if (reason === 'context') {
          cfg = await api(ctx.base, 'GET', '/api/config'); cfg.config.display.refresh_ms += 1;
          await api(ctx.base, 'PUT', '/api/config', {config: cfg.config, revision: cfg.revision});
          await page.evaluate(() => PAIGU_SHELL.refresh());
        }
        if (reason === 'api') await page.evaluate(() => {
          const url = new URL(location.href); url.searchParams.set('api', 'https://example.com'); history.replaceState(null, '', url);
        });
        if (reason === 'timeout') await page.waitForFunction(() => document.querySelector('#snap-download-info').textContent.includes('超时'));
        release(); await page.waitForTimeout(220);
        check(downloads.length === before, reason + ' blocks late download');
        check(!await page.locator('#snap-download-info').textContent().then(text => text.includes('已交给浏览器保存')), reason + ' cannot claim success');
      } finally {
        release(); await page.unroute('**/api/snapshot/download', route);
        if (reason === 'timeout') await page.evaluate(() => { window.setTimeout = window.backupTestTimeout; delete window.backupTestTimeout; });
      }
    }
    check(errors.length === 0, 'no page errors: ' + errors.join(';'));
  } finally {
    page.off('pageerror', onError); page.off('request', onRequest); page.off('download', onDownload);
    await api(ctx.base, 'POST', `/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`, {mode: 'fresh'});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
