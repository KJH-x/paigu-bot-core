import fs from 'node:fs';
import path from 'node:path';

function check(v,m) { if(!v) throw new Error(m); }
async function api(base,method,route,body,status=200) {
  const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(20000)});
  const data=await r.json(); check(r.status===status,`${method} ${route}: ${r.status}, expected ${status}`); return data;
}
export async function verifyMessageTransactions(page,ctx) {
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-transactions';
  const capture=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR) await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});};
  const errors=[],onError=e=>errors.push(String(e));page.on('pageerror',onError);
  const dialog=page.getByRole('dialog',{name:'修订消息动作'});
  try {
    check(!initial.config.llm.enabled&&!initial.config.gateway.reply_enabled,'isolated model and group actions');
    if(process.env.PAIGU_UI_CAPTURE_DIR) fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'补充与撤回验收',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{phases:[],priority_users:[],priority_window:null,items:[{item_id:'badge',name:'徽章',kind:'拼团',aliases:[],unit_price_cents:1000,subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]}]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    let seq=Date.now();
    const send=(text,member='成员01')=>api(ctx.base,'POST','/api/sim/messages',[{user_id:member,nickname:member,group_id:initial.config.round.group_id,text,timestamp:seq+=1000}]);
    const read=id=>api(ctx.base,'GET',`/api/messages/${id}/actions?round_id=${round}`);
    const count=async()=>{const b=await api(ctx.base,'GET','/api/board');return b.board.user_summaries.flatMap(u=>u.items).reduce((n,i)=>n+i.quantity,0);};
    await send('排 徽章 甲 1');await send('排 徽章 甲 2','成员02');await send('排 徽章 甲 2');
    const rows=(await api(ctx.base,'GET','/api/messages')).messages,main=rows[0],supp=rows[2];
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    const mainRow=page.locator('#messages .msg').filter({hasText:'成员01'}).filter({hasText:'排 徽章 甲 1'});
    await mainRow.waitFor();await mainRow.evaluate(n=>window.__txnOriginalNode=n);await mainRow.click();
    await dialog.locator('.ov-qty').fill('3');await dialog.locator('.ae-note').fill('后条是补充总数三份');
    await dialog.locator('.ae-supplements summary').click();
    const choice=dialog.locator('.ae-supplement').filter({hasText:'成员01：排 徽章 甲 2'});
    await choice.locator('input').check();
    check(await dialog.locator('.ae-supplements summary').textContent()==='补充消息 · 已选 1 条（选中条将停止重复执行）','selected supplement count');
    await capture('transaction-supplement-light');
    let response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith(`/api/messages/${main.record_id}/actions`));
    await dialog.locator('.ov-ok').click();check((await response).ok(),'atomic save succeeds');await dialog.waitFor({state:'detached'});
    check(await count()===5,'one replacement plus intervening other member, no duplicate supplement');
    check((await read(supp.record_id)).absorbed_by===main.record_id,'supplement points to main');
    await send('撤 徽章 1');
    const undo=page.locator('#undo-message-action');await undo.waitFor();await page.waitForFunction(()=>!document.querySelector('#undo-message-action').disabled);
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/messages/undo'));
    await undo.click();check((await response).ok(),'one-click inverse transaction');
    check(await count()===4,'new cancellation survives inverse transaction');
    check((await read(supp.record_id)).absorbed_by===null,'supplement restored together');
    check(await mainRow.evaluate(n=>window.__txnOriginalNode===n),'old message DOM identity survives save/undo');
    await capture('transaction-undone-light');
    // True automatic unit -> explicit group conversion, never a partial edit.
    let config=await api(ctx.base,'GET','/api/config');config.config.merge.enabled=true;
    await api(ctx.base,'PUT','/api/config',{config:config.config,revision:config.revision});
    await send('排 徽章 甲 1','成员03');await send('排 徽章 甲 2','成员03');
    await page.locator('#refresh').click();
    const groupRow=page.locator('#messages .msg').filter({hasText:'成员03'}).filter({hasText:'排 徽章 甲 1'});
    await groupRow.waitFor();
    if(await page.locator('html').getAttribute('data-theme')!=='dark')await page.locator('#theme-toggle').click();
    await groupRow.click();await dialog.locator('.ae-convert-group').click();
    await dialog.locator('.ov-qty').first().fill('4');
    if(await dialog.locator('.ae-remove-item').count()>1) await dialog.locator('.ae-remove-item').nth(1).click();
    check(await dialog.locator('.ov-qty').count()===1,'complete replacement leaves one item line');
    await capture('transaction-group-dark');
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().includes('/actions')&&!r.url().endsWith('/suggest'));
    await dialog.locator('.ov-ok').click();check((await response).ok(),'complete automatic group conversion');await dialog.waitFor({state:'detached'});
    check(await count()===8,'converted group executes only replacement');
    await page.waitForFunction(()=>!document.querySelector('#undo-message-action').disabled);
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/messages/undo'));
    await undo.click();check((await response).ok(),'group inverse succeeds');
    check(await count()===7,'group inverse restores original three pieces plus prior stream');
    // Browser-only mock: the actual server LLM remains disabled throughout.
    await page.route(/\/api\/messages\/[^/]+\/actions\?/,async route=>{const r=await route.fetch();const body=await r.json();body.model_draft_available=true;await route.fulfill({response:r,json:body});});
    let release=null,requests=0;
    await page.route(/\/api\/messages\/[^/]+\/actions\/suggest$/,async route=>{
      const body=route.request().postDataJSON();requests++;
      const selection=structuredClone(body.selection);selection.actions[0].items[0].quantity=6;
      if(requests===2) await new Promise(resolve=>{release=resolve;});
      await route.fulfill({json:{request_id:body.request_id,draft_version:body.draft_version,record_id:main.record_id,round_id:round,selection,executed:false}}).catch(()=>{});
    });
    await mainRow.click();await dialog.locator('.ae-suggest summary').click();
    await dialog.getByLabel('给模型的修订解释').fill('当时总共六份');
    const beforeCount=await count();await dialog.locator('.ae-suggest-button').click();
    await dialog.getByRole('status').filter({hasText:'已填入草稿'}).waitFor();
    check(await dialog.locator('.ov-qty').inputValue()==='6','explicit draft populates editable fields');
    check(await count()===beforeCount,'suggestion is not executed');
    await dialog.locator('.ae-suggest-button').click();
    for(let i=0;i<100&&!release;i++)await new Promise(r=>setTimeout(r,20));check(release,'delayed model request started');
    await dialog.locator('.ov-qty').fill('8');release();
    await dialog.getByRole('status').filter({hasText:'草稿已变化'}).waitFor();
    check(await dialog.locator('.ov-qty').inputValue()==='8','late response does not overwrite manual edits');
    await capture('transaction-ai-stale-dark');
    await page.keyboard.press('Escape');
    await page.locator('.draft-guard [data-choice="discard"]').click();
    check(requests===2,'only two explicit model button clicks issue requests');
    check(!errors.length,errors.join(';'));
  } finally {
    await page.unroute(/\/api\/messages\/[^/]+\/actions\?/);await page.unroute(/\/api\/messages\/[^/]+\/actions\/suggest$/);
    page.off('pageerror',onError);
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'fresh'});
    const c=await api(ctx.base,'GET','/api/config');c.config.merge.enabled=initial.config.merge.enabled;
    await api(ctx.base,'PUT','/api/config',{config:c.config,revision:c.revision});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
