import fs from 'node:fs';
import path from 'node:path';
function check(v,m){if(!v)throw new Error(m);}
async function api(base,method,route,body){const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const value=await r.json();check(r.ok,method+' '+route+': '+r.status);return value;}
export async function verifyPolicyReturn(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'), round='e2e-policy-return', errors=[], requests=[];
  const onError=e=>errors.push(String(e)), onRequest=r=>{if(r.method()!=='GET')requests.push(r.url());};
  page.on('pageerror',onError);
  const capture=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});}};
  const dialog=page.locator('.action-editor');let token='';
  try{
    const now=Date.now();
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'权限往返验收',copy_from:initial.config.round.round_id,category:'test'});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{priority_users:[],priority_window:{start_ms:now-60000,end_ms:now+600000},phases:[],items:[{item_id:'badge',name:'徽章',kind:'group',aliases:[],unit_price_cents:1000,subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]}]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    await api(ctx.base,'POST','/api/sim/messages',Array.from({length:35},(_,i)=>({user_id:i===10?'成员03':'成员02',nickname:i===10?'成员03':'成员02',group_id:initial.config.round.group_id,text:'排 徽章 甲 1',timestamp:now+i})));
    const records=(await api(ctx.base,'GET','/api/messages')).messages, target=records[10], route=`/api/messages/${target.record_id}/actions`;
    const original=await api(ctx.base,'GET',route+'?round_id='+round), beforeRevision=(await api(ctx.base,'GET','/api/config')).revision;
    check(original.current_outcome.status==='Rejected','target starts as a permission rejection');
    const auto=JSON.stringify(original.original_automatic);
    page.on('request',onRequest);
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#messages .msg').nth(34).waitFor();await page.locator('#message-filter').selectOption('Rejected');
    const targetRow=page.locator('#messages .msg').nth(10);
    await targetRow.evaluate(n=>n.parentElement.scrollTop=n.offsetTop-n.parentElement.offsetTop-80);
    await targetRow.click();await dialog.locator('.ov-qty').fill('3');await dialog.locator('.ae-note').fill('权限设置返回后的草稿');
    const scroll=await page.locator('#messages').evaluate(n=>n.scrollTop);
    await dialog.getByRole('button',{name:'管理优先名单与时段',exact:true}).click();
    await page.waitForFunction(r=>document.querySelector('#rd-id')?.value===r,round);
    await page.waitForFunction(()=>{var box=document.querySelector('#rd-priority').getBoundingClientRect();return box.y>0&&box.bottom<1080;});
    token=new URL(page.url()).searchParams.get('handoff');check(token&&!page.url().includes('成员')&&!page.url().includes('草稿'),'URL contains only opaque task token, not private draft');
    check((await api(ctx.base,'GET','/api/config')).revision===beforeRevision,'opening settings never writes or auto-adds priority');
    const onBackDialog=d=>d.accept();page.on('dialog',onBackDialog);
    try{await page.goBack();}finally{page.off('dialog',onBackDialog);}
    await dialog.locator('.ov-qty').waitFor();
    check(await dialog.locator('.ov-qty').inputValue()==='3'&&await dialog.locator('.ae-note').inputValue()==='权限设置返回后的草稿','browser Back also restores the held draft instead of opening an empty editor');
    await dialog.getByRole('button',{name:'管理优先名单与时段',exact:true}).click();
    await page.waitForFunction(r=>document.querySelector('#rd-id')?.value===r,round);token=new URL(page.url()).searchParams.get('handoff');
    await page.locator('#rd-priority').fill('成员03');
    let response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/config'));
    await page.locator('#save').click();check((await response).ok(),'priority saved through the real UI');
    await page.waitForFunction(()=>!document.querySelector('#save').disabled);
    let current=await api(ctx.base,'GET',route+'?round_id='+round);
    check(current.current_outcome.status==='Applied'&&current.effective_source==='saved_automatic','priority save recomputes existing actions, no override created');
    check(JSON.stringify(current.original_automatic)===auto,'original automatic rejection retained');
    await capture('policy-settings-return');
    await page.locator('.task-return').click();await dialog.locator('.ov-qty').waitFor();
    check(await dialog.locator('.ov-qty').inputValue()==='3'&&await dialog.locator('.ae-note').inputValue()==='权限设置返回后的草稿','unsaved action and explanation survive round trip');
    check(await page.locator('#message-filter').inputValue()==='Rejected','message filter restored');
    check(Math.abs(await page.locator('#messages').evaluate(n=>n.scrollTop)-scroll)<=1,'message scroll restored to the same location');
    check((await dialog.locator('.ae-original').textContent()).includes('已记录'),'returned editor shows freshly revalidated result');
    await capture('policy-returned-draft');
    await page.evaluate(()=>PAIGU_SHELL.theme.set('dark'));await page.waitForTimeout(200);await capture('policy-returned-draft-dark');
    response=page.waitForResponse(r=>r.request().method()==='POST'&&new URL(r.url()).pathname===route);
    await dialog.locator('.ov-ok').click();check((await response).ok(),'one direct save executes returned draft');await dialog.waitFor({state:'detached'});
    current=await api(ctx.base,'GET',route+'?round_id='+round);
    check(current.editable_selection.actions[0].items[0].quantity===3,'returned quantity executed');
    for(const key of ['seq','text','timestamp_ms','message_id','user_id'])check(current.message[key]===target[key],'message retains original '+key);
    check(await targetRow.isVisible(),'edited message remains pinned even when it leaves the rejection filter');
    await page.locator('#clear-message-location').click();check(!await targetRow.isVisible(),'explicit clear releases the out-of-filter pin');
    check(await page.evaluate(t=>!sessionStorage.getItem('paigu.private.task.'+t),token),'successful save clears private task draft');
    await page.locator('#wf-stepper a[href*="replay.html"]').click();
    await page.locator('#step-list .step-item').nth(10).click();
    await page.locator('#process-message:not([disabled])').waitFor();
    await page.locator('#process-message').focus();await page.keyboard.press('Space');await dialog.locator('.ae-note').waitFor();
    check(new URL(page.url()).searchParams.get('record')===target.record_id,'read-only replay links to the exact current record');
    // A later successful human revision invalidates the held old draft, not its original data.
    await dialog.locator('.ov-qty').fill('4');await dialog.locator('.ae-note').fill('版本冲突时保留的解释');
    await dialog.getByRole('button',{name:'管理阶段权限',exact:true}).click();
    await page.locator('.task-return').waitFor();token=new URL(page.url()).searchParams.get('handoff');
    current=await api(ctx.base,'GET',route+'?round_id='+round);
    await api(ctx.base,'POST',route,{transaction_id:'other-policy-return-edit',round_id:round,config_revision:current.config_revision,record_revision:current.message.record_revision,selection:{kind:'reject',note:'另一个管理操作'}});
    await page.locator('.task-return').click();await dialog.locator('.ae-content').filter({hasText:'以下旧草稿只读保留'}).waitFor();
    check(await dialog.locator('.ov-ok').isDisabled()&&(await dialog.textContent()).includes('版本冲突时保留的解释'),'stale draft remains reviewable but cannot overwrite a newer choice');
    check((await api(ctx.base,'GET',route+'?round_id='+round)).current_outcome.status==='Rejected','stale return did not execute anything');
    await capture('policy-return-stale');await dialog.locator('.ov-cancel').click();
    check(await page.evaluate(t=>!!sessionStorage.getItem('paigu.private.task.'+t),token),'conflicting draft is not silently erased');
    await page.evaluate(t=>PAIGU.clearTask(t),token);
    const replay=await api(ctx.base,'GET','/api/replay/view');
    await page.locator('#wf-stepper a[href*="replay.html"]').click();
    await page.locator('#file-input').setInputFiles({name:'placeholder-replay.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(replay))});
    await page.locator('#source-info').filter({hasText:'导入文件'}).waitFor();
    check(await page.locator('#process-message').isDisabled(),'even an imported replay containing matching local record IDs remains read-only');
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}&record=${target.record_id}&message_round=other-round`);
    await page.locator('#message-location-info').filter({hasText:'另一轮次'}).waitFor();check(await dialog.count()===0,'cross-round deep link never silently activates or edits');
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}&source=remote&remote=${encodeURIComponent(ctx.remote.base)}&round=${encodeURIComponent('月行水上')}&record=${target.record_id}`);
    await page.locator('#message-location-info').filter({hasText:'来源只读'}).waitFor();check(await dialog.count()===0,'read-only remote source never opens local action editing');
    check(!requests.some(u=>/\/suggest|\/cn-suggest/.test(u)),'no model request from opening, editing, settings save, return or execution');
    check(errors.length===0,'no browser errors: '+errors.join(';'));
  }finally{
    page.off('pageerror',onError);page.off('request',onRequest);
    if(await page.locator('.draft-guard').count())await page.locator('.draft-guard [data-choice="discard"]').click();
    if(await dialog.count()){await dialog.locator('.ov-cancel').click();if(await page.locator('.draft-guard').count())await page.locator('.draft-guard [data-choice="discard"]').click();}
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'fresh'});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
