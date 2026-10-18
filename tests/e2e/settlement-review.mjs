import fs from 'node:fs';
import path from 'node:path';

function check(value,message){if(!value)throw new Error(message);}
async function api(base,method,route,body,status=200){
  const res=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(20000)});
  const data=await res.json();check(res.status===status,`${method} ${route}: ${res.status} expected ${status}: ${JSON.stringify(data).slice(0,200)}`);return data;
}
export async function verifySettlementReview(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-settlement-review';
  const view=()=>api(ctx.base,'GET','/api/settlement/draft');
  const saved=()=>page.waitForFunction(()=>document.querySelector('#draft-status').textContent.includes('已保存'));
  const refresh=()=>page.evaluate(()=>window.PAIGU_SHELL.refresh());
  const theme=async value=>{
    await page.evaluate(async t=>{window.PAIGU_SHELL.theme.set(t);await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);await Promise.all(document.getAnimations().filter(a=>Number.isFinite(a.effect.getComputedTiming().endTime)).map(a=>a.finished.catch(()=>{})));},value);
    const contrast=await page.locator('#order-numbers input').first().evaluate(input=>{
      const css=getComputedStyle(input),rgb=s=>s.match(/[\d.]+/g).slice(0,3).map(Number);
      const lum=c=>c.map(x=>{x/=255;return x<=.04045?x/12.92:((x+.055)/1.055)**2.4;}).reduce((v,x,i)=>v+x*[.2126,.7152,.0722][i],0);
      const a=lum(rgb(css.color)),b=lum(rgb(css.backgroundColor));return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);
    });check(contrast>=4.5,value+' order input contrast must be readable after actual transitions: '+contrast);
  };
  const shot=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});}};
  let locked=false,gate=null,releaseGate=null;
  try{
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'订单核对占位',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{phases:[],priority_users:[],priority_window:null,items:[
      {item_id:'badge',name:'徽章',kind:'group',class:'B',aliases:[],unit_price_cents:1000,subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]},
      {item_id:'unused',name:'无人认领占位',kind:'group',aliases:[],unit_price_cents:0,subitems:[]}
    ]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    await api(ctx.base,'POST','/api/sim/messages',[{user_id:'成员01',nickname:'成员01',group_id:initial.config.round.group_id,text:'排 徽章 甲 2',timestamp:Date.now()}]);
    await page.goto(`${ctx.base}/settlement.html?api=${encodeURIComponent(ctx.base)}#order`);
    await page.locator('#load-allocation:not(:disabled)').waitFor();
    let generatedResponse=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/settlement/from-allocation'));
    await page.locator('#load-allocation').click();check((await generatedResponse).ok(),'real UI generates the current allocation');await saved();
    const numberInputs=page.locator('#order-numbers input');
    check(await numberInputs.count()>0,'generation exposes actual package order numbers');
    for(let i=0;i<await numberInputs.count();i++){await numberInputs.nth(i).fill('GENERATED-'+i);await saved();}
    let lockResponse=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/settlement/lock'));
    await page.locator('#lock-order').click();check((await lockResponse).ok(),'generated order locks despite unclaimed zero-price catalog entry');locked=true;
    lockResponse=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/settlement/unlock'));
    page.once('dialog',d=>d.accept());
    await page.locator('#unlock-order').click();check((await lockResponse).ok(),'same UI unlocks the generated order');locked=false;
    await page.waitForFunction(()=>document.querySelector('#unlock-order').classList.contains('hidden')&&!document.querySelector('#load-allocation').disabled);
    await page.goto(`${ctx.base}/index.html?api=${encodeURIComponent(ctx.base)}`);
    let v=await view();
    const line={item_id:'badge',subitem_id:'a',qty:1,unit_price_cents:1000,is_gift:false};
    const table={packages:[{package_id:'空包',lines:[]},{package_id:'包甲',lines:[{...line,unit_price_cents:0}]},{package_id:'包乙',lines:[{...line,qty:2}]}]};
    await api(ctx.base,'PUT','/api/settlement/draft',{round_id:round,revision:v.revision,source_token:v.source_token,order_table:table,order_numbers:{'包乙':'SAME'}});
    await page.goto(`${ctx.base}/settlement.html?api=${encodeURIComponent(ctx.base)}#order`);
    await page.locator('#order-issues button[data-issue="order_missing"]').waitFor();
    check(await page.locator('#load-sample').count()===0,'production has no sample-to-live-draft writer');
    await page.locator('#order-issues button[data-issue="order_missing"]').focus();await page.keyboard.press('Enter');
    check(await page.evaluate(()=>document.activeElement.getAttribute('aria-label'))==='包甲 的订单号','missing order issue targets correct original package index after empty package');
    await page.keyboard.type('SAME');await saved();
    await page.locator('#order-issues button[data-issue="order_duplicate"]').focus();await page.keyboard.press('Space');
    check(await page.evaluate(()=>document.activeElement.getAttribute('aria-label'))==='包乙 的订单号','duplicate order target is reachable');
    await page.keyboard.press('Control+A');await page.keyboard.type('ORDER-02');await saved();
    await page.getByRole('textbox',{name:'包甲 的订单号',exact:true}).fill('ORDER-01');await saved();
    await page.locator('#order-issues button[data-issue="price_missing"]').click();
    check(await page.evaluate(()=>document.activeElement.dataset.field)==='price','price issue focuses price input');
    await page.keyboard.press('Control+A');await page.keyboard.type('10');await saved();
    await page.locator('#order-issues button[data-issue="quantity_gap"]').click();
    check(await page.evaluate(()=>document.activeElement.dataset.field)==='quantity','quantity discrepancy is locatable');
    await page.locator('#order-table [data-package-index="2"] [data-field="quantity"]').fill('1');await saved();
    check((await view()).issues.length===0,'all corrected issues disappear after real saved validation');

    // A rule save is scoped and invalidates review, not the manually edited order.
    const before=(await view()).draft;
    const rulesBefore=(await api(ctx.base,'GET','/api/config')).config.settlement;
    await page.locator('#reduce-include').setChecked(!await page.locator('#reduce-include').isChecked());
    let config=await api(ctx.base,'GET','/api/config');config.config.display.refresh_ms+=1;
    await api(ctx.base,'PUT','/api/config',{config:config.config,revision:config.revision});
    let response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/config'));
    await page.locator('#save').click();check((await response).ok(),'settlement rule save remains scoped');
    await page.waitForFunction(()=>document.querySelector('#review-status').textContent.includes('需要重新核对'));
    v=await view();
    check(v.needs_review&&v.result_stale,'rule changes mark old trial and review stale');
    check(JSON.stringify(v.draft.order_table)===JSON.stringify(before.order_table),'rule changes do not rebuild packages');
    check(JSON.stringify(v.draft.order_numbers)===JSON.stringify(before.order_numbers),'rule changes do not clear numbers');
    check(await page.locator('#lock-order').isDisabled(),'review is required before lock');
    await theme('dark');await shot('settlement-review-dark');
    await theme('light');await shot('settlement-review-light');
    await theme('dark');
    response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/settlement/draft'));
    await page.locator('#review-order').click();const reviewed=await (await response).json();
    check(reviewed.needs_review===false,'explicit atomic review');
    check((await api(ctx.base,'GET','/api/config')).config.display.refresh_ms===config.config.display.refresh_ms,'independent display changes are preserved by rule save');
    check(JSON.stringify((await view()).draft.order_numbers)===JSON.stringify(before.order_numbers),'explicit review preserves numbers');

    // The response to a delayed autosave must not accept a newly changed source.
    let started;const waiting=new Promise(resolve=>{started=resolve;});
    gate=new Promise(resolve=>{releaseGate=resolve;});
    await page.route('**/api/settlement/draft',async route=>{if(route.request().method()==='PUT'){started();await gate;}await route.continue();});
    await page.getByRole('textbox',{name:'包甲 的订单号',exact:true}).fill('ORDER-NEW');await Promise.race([waiting,new Promise((_,reject)=>setTimeout(()=>reject(new Error('delayed autosave did not start')),10000))]);
    config=await api(ctx.base,'GET','/api/config');config.config.round.priority_users=['成员02'];
    await api(ctx.base,'PUT','/api/config',{config:config.config,revision:config.revision});
    response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/settlement/draft'));
    releaseGate();gate=null;check((await response).status()===409,'in-flight stale source rejected');await page.unroute('**/api/settlement/draft');
    check(await page.getByRole('textbox',{name:'包甲 的订单号',exact:true}).inputValue()==='ORDER-NEW','failed save retains input');
    await refresh();await page.waitForFunction(()=>!document.querySelector('#review-order').hidden);
    response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/settlement/draft'));
    await page.locator('#review-order').click();check((await response).ok(),'explicit recheck retries retained input against current source');await saved();
    check((await view()).draft.order_numbers['包甲']==='ORDER-NEW','retained input persists after explicit review');

    await api(ctx.base,'POST','/api/sim/messages',[{user_id:'成员01',nickname:'成员01',group_id:initial.config.round.group_id,text:'撤 徽章 甲 1',timestamp:Date.now()+1000}]);
    await refresh();await page.waitForFunction(()=>!document.querySelector('#review-order').hidden);
    response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/settlement/draft'));
    await page.locator('#review-order').click();check((await response).ok(),'review keeps order rather than silently resizing after cancellation');await saved();
    await page.locator('#order-issues button[data-issue="quantity_gap"]').waitFor();
    await shot('settlement-quantity-issue');
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/settlement/lock'));
    await page.locator('#lock-order').click();check((await response).status()===422,'review alone cannot bypass actual quantity validation');
    await page.locator('#order-table [data-package-index="2"] [data-field="quantity"]').fill('0');await saved();
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/settlement/lock'));
    await page.locator('#lock-order').click();check((await response).ok(),'correct reviewed order locks');locked=true;
    v=await view();await api(ctx.base,'POST','/api/settlement/unlock',{round_id:round,revision:v.revision});locked=false;
    await page.route('**/api/settlement/draft',async route=>{if(route.request().method()==='GET')await route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:'isolated read failure'})});else await route.continue();});
    await page.reload();await page.waitForFunction(()=>document.querySelector('#review-status').textContent.includes('读取失败'));
    check(await page.locator('#lock-order').isDisabled(),'failed draft read cannot masquerade as reviewed or allow lock');
    check(await page.locator('#pkg-add').isDisabled(),'failed draft read cannot create an overwrite draft');
    check(await page.locator('#wf-stepper a').count()===7,'navigation remains reachable while draft is unavailable');
    await page.unroute('**/api/settlement/draft');await page.locator('#load').click();await saved();
    check((await api(ctx.base,'GET','/api/config')).config.llm.enabled===false,'all reconciliation stays deterministic without real model');
    // Restoring global rule config is an explicit isolated cleanup, never a UI side effect.
    config=await api(ctx.base,'GET','/api/config');config.config.settlement=rulesBefore;
    await api(ctx.base,'PUT','/api/config',{config:config.config,revision:config.revision});
  }catch(error){
    const state=await page.evaluate(()=>({draft:document.querySelector('#draft-status')?.textContent,review:document.querySelector('#review-status')?.textContent,banner:document.querySelector('#banner')?.textContent,issues:document.querySelector('#order-issues')?.textContent}));
    error.message+=' '+JSON.stringify(state);await shot('settlement-test-failure');throw error;
  }finally{
    if(releaseGate)releaseGate();
    await page.unroute('**/api/settlement/draft');
    if(locked){const v=await view();await api(ctx.base,'POST','/api/settlement/unlock',{round_id:round,revision:v.revision});}
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'fresh'});
    const config=await api(ctx.base,'GET','/api/config');config.config.settlement=initial.config.settlement;config.config.display=initial.config.display;
    await api(ctx.base,'PUT','/api/config',{config:config.config,revision:config.revision});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
