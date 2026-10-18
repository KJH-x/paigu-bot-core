import path from 'node:path';
function check(v,m){if(!v)throw new Error(m);}
async function api(base,method,route,body){
  const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(20000)});
  const data=await r.json();check(r.ok,`${method} ${route}: ${r.status} ${JSON.stringify(data).slice(0,160)}`);return data;
}
export async function verifyDraftConcurrency(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-draft-boundaries',pending=[];
  const dialog=page.getByRole('dialog',{name:'修订消息动作'});
  const status=fragment=>dialog.getByRole('status').filter({hasText:fragment});
  const contextRoute=/\/api\/messages\/[^/]+\/actions\?/;
  const suggestRoute=/\/api\/messages\/[^/]+\/actions\/suggest$/;
  let failure=null,locked=false,records;
  async function close(){
    await dialog.locator('.ov-cancel').click();
    const guard=page.locator('.draft-guard [data-choice="discard"]');
    if(await guard.isVisible())await guard.click();
    await dialog.waitFor({state:'detached'});
  }
  async function open(index=0){
    await page.locator('#messages .msg').filter({hasText:records[index].text}).click();
    await dialog.locator('.ae-suggest summary').click();
    await dialog.getByLabel('给模型的修订解释').fill('这里原意为三份');
  }
  async function request(){
    const count=pending.length;await dialog.locator('.ae-suggest-button').click();
    for(let i=0;i<100&&pending.length===count;i++)await page.waitForTimeout(20);
    check(pending.length===count+1,'one explicit click starts one request');return pending.at(-1);
  }
  try{
    check(!initial.config.llm.enabled&&!initial.config.gateway.reply_enabled,'no real model or group action');
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'代填并发占位',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{phases:[],priority_users:[],priority_window:null,items:[{item_id:'badge',name:'徽章',kind:'group',aliases:[],unit_price_cents:1000,subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]}]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    await api(ctx.base,'POST','/api/sim/messages',[1,2].map((quantity,i)=>({user_id:'成员0'+(i+1),nickname:'成员0'+(i+1),group_id:initial.config.round.group_id,text:`排 徽章 甲 ${quantity}`,timestamp:Date.now()+i*1000})));
    records=(await api(ctx.base,'GET','/api/messages')).messages;
    await page.route(contextRoute,async route=>{const response=await route.fetch();const data=await response.json();data.model_draft_available=true;await route.fulfill({response,json:data});});
    await page.route(suggestRoute,async route=>{
      const body=route.request().postDataJSON();
      const entry={body,release:null,quantity:6};pending.push(entry);
      if(failure){await route.fulfill(failure);return;}
      await new Promise(resolve=>{entry.release=resolve;});
      const selection=structuredClone(body.selection);selection.actions[0].items[0].quantity=entry.quantity;
      await route.fulfill({json:{request_id:body.request_id,draft_version:body.draft_version,record_id:new URL(route.request().url()).pathname.split('/')[3],round_id:body.round_id,selection,executed:false}}).catch(()=>{});
    });
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await open();check(pending.length===0,'opening, expanding and typing do not request a model');
    let first=await request();await dialog.getByRole('button',{name:'取消代填请求'}).click();first.release();
    await status('已停止等待').waitFor();check(await dialog.locator('.ov-qty').inputValue()==='1','cancel leaves draft unchanged');
    first=await request();const second=await request();second.quantity=7;second.release();
    await status('已填入草稿').waitFor();first.release();
    await page.waitForTimeout(100);check(await dialog.locator('.ov-qty').inputValue()==='7','newer explicit request wins over late older request');
    await close();await open();first=await request();await close();await open(1);first.release();
    await page.waitForTimeout(100);check(await dialog.locator('.ov-qty').inputValue()==='2','late response cannot fill a different record');await close();
    await open();first=await request();
    const current=await api(ctx.base,'GET',`/api/messages/${records[0].record_id}/actions?round_id=${round}`);
    await api(ctx.base,'POST',`/api/messages/${records[0].record_id}/actions`,{transaction_id:crypto.randomUUID(),round_id:round,config_revision:current.config_revision,record_revision:current.message.record_revision,selection:{...current.editable_selection,note:'另一个编辑者已修订'},supplements:[],release_supplements:[]});
    first.release();await status('上下文或锁定状态已变化').waitFor();check(await dialog.locator('.ov-qty').inputValue()==='1','record revision change rejects late draft');await close();
    await open();first=await request();
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'continue'});
    first.release();await status('代填未完成').waitFor();
    check(await dialog.locator('.ov-qty').inputValue()==='1','round switch cannot apply a late draft to another round');await close();
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'continue'});
    await page.reload();
    // Controlled transport failures, never actual external inference.
    for(const reply of [{status:504,json:{error:'controlled timeout'}},{status:200,contentType:'application/json',body:'not json'}]){
      failure=reply;await open();await request();await status('代填未完成').waitFor();
      await dialog.locator('.ov-qty').fill('4');check(await dialog.locator('.ov-ok').isEnabled(),'failure retains available manual editor');await close();
    }
    failure=null;await open();
    await page.evaluate(()=>{const p=window.PAIGU,original=p.post;p.post=(url,body,opts)=>original(url,body,url.endsWith('/suggest')?{...opts,timeout:80}:opts);});
    first=await request();await status('代填未完成').waitFor();first.release();
    check(await dialog.locator('.ov-qty').inputValue()==='1','transport timeout leaves manual draft unchanged');await close();
    // The lock is real and isolated; UI assistance remains browser-only mock.
    await page.reload();await open();first=await request();
    let view=await api(ctx.base,'GET','/api/settlement/draft');
    const generated=await api(ctx.base,'POST','/api/settlement/from-allocation',{});
    const table=generated.order_table || generated.draft?.order_table;
    check(table?.packages.length>0,'isolated current allocation generates real order packages');
    await api(ctx.base,'PUT','/api/settlement/draft',{round_id:round,revision:view.revision,source_token:view.source_token,order_table:table,order_numbers:Object.fromEntries(table.packages.map((p,i)=>[p.package_id,'DRAFT-'+i]))});
    view=await api(ctx.base,'GET','/api/settlement/draft');
    await api(ctx.base,'POST','/api/settlement/lock',{round_id:round,revision:view.revision,source_token:view.source_token});locked=true;
    first.release();await status('上下文或锁定状态已变化').waitFor();check(await dialog.locator('.ov-qty').inputValue()==='1','lock discards late response');await close();
    await page.locator('#messages .msg').filter({hasText:records[0].text}).click();
    await dialog.locator('.ae-warning').filter({hasText:'轮次已锁定'}).waitFor();
    check(await dialog.locator('.ov-ok').isDisabled()&&await dialog.locator('.ae-suggest-button').count()===0,'locked record opens read-only without misleading editable controls');
    await page.evaluate(()=>window.PAIGU_SHELL.refresh());await page.locator('#wf-locked').filter({hasText:'锁定：是'}).waitFor();
    if(process.env.PAIGU_UI_CAPTURE_DIR)await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,'message-locked-readonly.png')});
    await close();
    check(!(await api(ctx.base,'GET','/api/config')).config.llm.enabled,'actual model stayed disabled');
  }finally{
    pending.forEach(p=>p.release?.());
    await page.unroute(contextRoute);await page.unroute(suggestRoute);
    if(locked){const v=await api(ctx.base,'GET','/api/settlement/draft');await api(ctx.base,'POST','/api/settlement/unlock',{round_id:round,revision:v.revision,source_token:v.source_token});}
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'continue'});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
