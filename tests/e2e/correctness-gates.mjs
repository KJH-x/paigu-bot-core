import fs from 'node:fs';
import path from 'node:path';

function check(v,m){if(!v)throw new Error(m);}
async function api(base,method,route,body,status=200){
  const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(20000)});
  const data=await r.json();check(r.status===status,`${method} ${route}: ${r.status}, expected ${status}: ${JSON.stringify(data).slice(0,160)}`);return data;
}
export async function verifyCorrectnessGates(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-correctness';
  const capture=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});}};
  const draft=async()=>{const view=await api(ctx.base,'GET','/api/settlement/draft');view.draft.source_token=view.source_token;return view;};
  let locked=false;
  try{
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'正确性门禁',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{phases:[],priority_users:[],priority_window:null,items:[
      {item_id:'badge',name:'徽章',kind:'group',class:'B',aliases:[],unit_price_cents:1000,box_item_id:'whole',subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]},
      {item_id:'whole',name:'整盒占位',kind:'box',aliases:[],unit_price_cents:5000,subitems:[]},
      {item_id:'unused',name:'无人认领占位',kind:'group',aliases:[],unit_price_cents:0,subitems:[{subitem_id:'b',name:'乙',aliases:[],capacity:1,unit_price_cents:0,adjust_cents:0}]}
    ]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    await api(ctx.base,'POST','/api/sim/messages',[{user_id:'成员01',nickname:'成员01',group_id:initial.config.round.group_id,text:'排 徽章 甲 2',timestamp:Date.now()}]);
    await page.goto(`${ctx.base}/round.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#rd-title').fill('正确性门禁更新');
    let response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/config'));
    await page.locator('#save').click();check((await response).ok(),'round edit saves');
    const afterRound=await api(ctx.base,'GET','/api/config');
    check(afterRound.config.round.items.find(i=>i.item_id==='badge').box_item_id==='whole','unrelated round edit preserves box association');
    check(afterRound.config.round.items.find(i=>i.item_id==='badge').class==='B','unrelated edits preserve an explicit advanced class instead of silently deriving A');
    check(await page.getByRole('combobox',{name:'徽章的整盒关联',exact:true}).inputValue()==='whole','existing box association is editable without showing IDs as the main label');
    // The bundled example deliberately has no real IDs. Editing requires a
    // stable identity, so use an isolated cache rather than inventing one in UI.
    fs.writeFileSync(afterRound.config.members.cache_path,JSON.stringify({members:[
      {user_id:'成员01',card:'成员01 占位备注',nickname:'成员01'},
      {user_id:'成员02',card:'成员02',nickname:'成员02'}
    ]}));
    await page.goto(`${ctx.base}/settings.html?api=${encodeURIComponent(ctx.base)}#sec-members`);
    const cn=page.locator('#members-body .cn').first();await cn.waitFor();
    check(new URL(page.url()).pathname.endsWith('/members.html'),'old member anchor reaches the independent member workspace');
    check(await page.locator('#members-body .clean').count()===0,'only final CN is editable');
    const memberId=await cn.getAttribute('data-user');
    await cn.fill('');await cn.pressSequentially('测试称呼甲');
    check(await cn.inputValue()==='测试称呼甲','per-character input is not reset by rendering');
    await cn.fill('');await cn.focus();
    const cdp=await page.context().newCDPSession(page);
    try{
      await cdp.send('Input.imeSetComposition',{text:'中',selectionStart:1,selectionEnd:1});
      await cdp.send('Input.imeSetComposition',{text:'中文',selectionStart:2,selectionEnd:2});
      await cdp.send('Input.insertText',{text:'中文输入甲'});
      check(await cn.inputValue()==='中文输入甲','actual browser IME composition is committed without losing characters or focus');
      await cn.fill('');
      // Controlled paste data never reads or overwrites the system clipboard.
      await cn.evaluate(n=>{const data=new DataTransfer();data.setData('text/plain','粘贴称呼乙');n.dispatchEvent(new ClipboardEvent('paste',{clipboardData:data,bubbles:true,cancelable:true}));});
      await cdp.send('Input.insertText',{text:'粘贴称呼乙'});
      check(await cn.inputValue()==='粘贴称呼乙','paste path retains the full controlled text');
      await cn.fill('');await cn.pressSequentially('测试称呼甲');
      check(await cn.evaluate(n=>n===document.activeElement),'clear/retype retains the same focused control');
    }finally{await cdp.detach();}
    await page.locator('#member-source').evaluate(n=>n.open=true);
    await page.locator('#mb-cache').fill(path.join(ctx.tmpDir,'unsaved-source.json'));
    response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/config'));
    await page.locator('#members-cn-save').click();check((await response).ok(),'scoped CN save');
    let saved=await api(ctx.base,'GET','/api/config');
    check(saved.config.members.cn_overrides.find(m=>m.user_id===memberId).cn==='测试称呼甲','all typed CN characters are retained');
    check(saved.config.gateway.bind===afterRound.config.gateway.bind,'CN save does not commit unrelated gateway draft');
    check(saved.config.members.cache_path===afterRound.config.members.cache_path,'CN save does not commit member-source draft');
    const row=cn.locator('..').locator('..');await row.locator('button[data-use]').click();
    await page.locator('#members-cn-save').click();await page.waitForFunction(()=>!document.querySelector('#members-cn-save').disabled);
    saved=await api(ctx.base,'GET','/api/config');
    check(!saved.config.members.cn_overrides.some(m=>m.user_id===memberId&&m.cn),'restore automatic removes frozen CN');
    await capture('g0-member-single-source');
    await page.locator('#load').click();
    await page.locator('.draft-guard [data-choice="cancel"]').click();
    check((await page.locator('#mb-cache').inputValue()).endsWith('unsaved-source.json'),'cancel re-reading keeps the separate source draft');
    await page.locator('#load').click();
    await page.locator('.draft-guard [data-choice="discard"]').click();
    await page.waitForFunction(expected=>document.querySelector('#mb-cache').value===expected,afterRound.config.members.cache_path);
    // Two equivalent moves: native pointer drag and a named click/keyboard control.
    const line={item_id:'badge',subitem_id:'a',qty:1,unit_price_cents:1000,is_gift:false};
    const table={packages:[{package_id:'包甲',lines:[line]},{package_id:'包乙',lines:[structuredClone(line)]}]};
    let d=(await draft()).draft;
    await api(ctx.base,'PUT','/api/settlement/draft',{round_id:round,revision:d.revision,source_token:d.source_token,order_table:table,order_numbers:{'包甲':'ORDER-01','包乙':'ORDER-02'}});
    await page.goto(`${ctx.base}/settlement.html?api=${encodeURIComponent(ctx.base)}#order`);
    await page.locator('#order-table .line-handle').first().waitFor();
    await page.locator('#order-table .line-handle').first().dragTo(page.locator('#order-table .pkg').nth(1));
    await page.waitForFunction(()=>document.querySelector('#order-table .pkg:first-child .line')===null);
    await page.waitForFunction(()=>document.querySelector('#draft-status').textContent.includes('已保存'));
    const dragged=(await draft()).draft.order_table;
    check(dragged.packages[1].lines[0].qty===2,'pointer drag moves full quantity');
    d=(await draft()).draft;
    await api(ctx.base,'PUT','/api/settlement/draft',{round_id:round,revision:d.revision,source_token:d.source_token,order_table:table,order_numbers:{'包甲':'ORDER-01','包乙':'ORDER-02'}});
    await page.locator('#load').click();await page.waitForFunction(()=>document.querySelectorAll('#order-table .line').length===2);
    await page.locator('#order-table .line-move select').first().selectOption('1');
    await page.locator('#order-table .line-move button').first().click();
    await page.waitForFunction(()=>document.querySelector('#order-table .pkg:first-child .line')===null);
    await page.waitForFunction(()=>document.querySelector('#draft-status').textContent.includes('已保存'));
    check(JSON.stringify((await draft()).draft.order_table)===JSON.stringify(dragged),'click move and drag produce identical order table');
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/settlement/lock'));
    await page.locator('#lock-order').click();check((await response).ok(),'unclaimed zero-price catalog items do not block lock');locked=true;
    await page.locator('#unlock-order:not(.hidden)').waitFor();
    check(await page.locator('#order-table .line-handle').evaluateAll(ns=>ns.every(n=>!n.draggable)),'locked handles are not draggable');
    check(await page.locator('#order-table .line-move button').first().isDisabled(),'locked alternate move is disabled');
    d=(await draft()).draft;
    await api(ctx.base,'PUT','/api/settlement/draft',{round_id:round,revision:d.revision,source_token:d.source_token,order_table:table,order_numbers:d.order_numbers},423);
    await capture('g0-locked-order');
    await api(ctx.base,'POST','/api/settlement/unlock',{round_id:round,revision:d.revision});locked=false;
    // A preview must be explicit, stale previews must never overwrite new input.
    await page.goto(`${ctx.base}/settings.html?api=${encodeURIComponent(ctx.base)}#sec-snapshot`);
    await page.locator('#snap-out').fill(path.join(ctx.tmpDir,'private-backup.snapshot.json'));
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/snapshot/export'));
    await page.locator('#snap-export').click();const exported=await (await response).json();
    await page.waitForFunction(()=>!!document.querySelector('#snap-path').value);
    check(await page.locator('#snap-apply').isDisabled(),'application unavailable before preflight');
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/snapshot/import'));
    await page.locator('#snap-import').click();const checked=await (await response).json();
    check(checked.applied===false&&checked.applicable,'preflight does not apply');
    await page.waitForFunction(()=>!document.querySelector('#snap-apply').disabled);
    await capture('private-backup-preflight');
    await api(ctx.base,'POST','/api/sim/messages',[{user_id:'成员02',nickname:'成员02',group_id:initial.config.round.group_id,text:'占位新消息',timestamp:Date.now()+1000}]);
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/snapshot/import'));
    await page.locator('#snap-apply').click();check((await response).status()===409,'new message invalidates backup preflight');
    check((await api(ctx.base,'GET','/api/messages')).messages.length===2,'stale application cannot delete new input');
    check(fs.existsSync(exported.path),'backup is a local file, not a public upload');
  }finally{
    fs.rmSync(initial.config.members.cache_path,{force:true});
    if(locked){const d=(await draft()).draft;await api(ctx.base,'POST','/api/settlement/unlock',{round_id:round,revision:d.revision});}
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'fresh'});
    const c=await api(ctx.base,'GET','/api/config');c.config.members.cn_overrides=initial.config.members.cn_overrides;
    await api(ctx.base,'PUT','/api/config',{config:c.config,revision:c.revision});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
