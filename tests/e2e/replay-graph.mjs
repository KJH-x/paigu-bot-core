import fs from 'node:fs';
import path from 'node:path';

function check(value,message){if(!value)throw new Error(message);}
const longName='超长姓名成员ABCDEFGHIJKLMNOPQRSTUVWXYZ汉字混排 "引号" <安全> #35; #quot;';
function fixture(){
  const items=[{item_id:'badge',name:'徽章',kind:'group',subitems:[{subitem_id:'a',name:'款式甲',capacity:2},{subitem_id:'b',name:'款式乙',capacity:2}]},{item_id:'stand',name:'立牌',kind:'single',subitems:[]},{item_id:'gift',name:'特典',kind:'gift',subitems:[]}];
  const names=Array.from({length:36},(_,i)=>i===0?longName:'占位成员'+String(i).padStart(2,'0'));
  const board=alternate=>({item_allocations:[{item_id:'badge',subitem_id:'a',kind:'group',boxes:names.map((name,i)=>({box_index:i+1,slots:[{slot_index:1,user_id:'u'+i,status:'Filled'},{slot_index:2,user_id:i%2?'u'+i:null,status:i%2?'Filled':'Empty'}]})),singles:[{user_id:'single',quantity:2}],waiting:[{user_id:'waiting',quantity:3}]},{item_id:'badge',subitem_id:'b',kind:'group',boxes:[{box_index:1,slots:[{slot_index:1,user:alternate?'占位乙':'占位甲',status:'Filled'}]}],singles:[],waiting:[]},{item_id:'stand',subitem_id:null,kind:'single',boxes:[],singles:[{user_id:'single',quantity:5}],waiting:[]},{item_id:'gift',subitem_id:null,kind:'gift',boxes:[],singles:[{user_id:'single',quantity:1}],waiting:[]}],user_summaries:names.map((name,i)=>({user_id:'u'+i,display_name:name})).concat([{user_id:'single',display_name:'单领成员'},{user_id:'waiting',display_name:'排队成员'}])});
  return {round_id:'graph-placeholder',title:'关系图占位核对',items,messages:[{seq:1,display:'占位成员01',text:'排谷占位',status:'Applied'},{seq:2,display:'占位成员02',text:'修改占位',status:'Applied'}],steps:[{message_seq:1,board:board(false),changed:[]},{message_seq:2,board:board(true),changed:[{item_id:'badge',subitem_id:'b',box_index:1,slot:1,before:'占位甲',after:'占位乙'}]}]};
}
export async function verifyReplayGraph(page,ctx){
  const errors=[],writes=[];const onError=e=>errors.push(e.message),onRequest=r=>{if(['POST','PUT','DELETE'].includes(r.method())&&r.url().startsWith(ctx.base+'/api/'))writes.push(r.url());};
  page.on('pageerror',onError);page.on('request',onRequest);
  const shot=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});}};
  const ready=()=>page.waitForFunction(()=>document.querySelectorAll('#mermaid-view svg').length>0 && Array.from(document.querySelectorAll('.graph-canvas')).every(n=>n.getAttribute('aria-busy')==='false'));
  try{
    await page.goto(`${ctx.base}/replay.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#file-input').setInputFiles({name:'graph-placeholder.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(fixture()))});
    await page.waitForFunction(()=>document.querySelector('#source-info').textContent.includes('导入文件'));
    await page.locator('#btn-last').click();await page.locator('#tab-mermaid').click();await ready();
    check(await page.locator('#graph-item').inputValue()==='badge','default graph focuses current changed product');
    check(await page.locator('.graph-card').count()===1,'default does not squeeze all products into one giant graph');
    const long=page.locator('.graph-node').filter({has:page.locator('title',{hasText:longName})});
    check(await long.count()===1,'special characters and long mixed name preserved in full title');
    check(await long.getAttribute('aria-label')===longName,'full accessible graph name');
    check((await page.locator('#mermaid-view').textContent()).includes('单领成员 · 单领 ×2'),'raw user_id single resolves through current snapshot names');
    check((await page.locator('#mermaid-view').textContent()).includes('排队成员 · 等待 ×3'),'waiting queue is no longer silently missing');
    const card=await page.locator('.graph-card').evaluate(n=>{window.graphCard=n;return n.dataset.item;});check(card==='badge','stable product card identity');
    await page.locator('#graph-item').selectOption('');await ready();
    check(await page.locator('.graph-card').count()===3,'all products remain separately grouped with headings');
    const ids=await page.locator('#mermaid-view marker').evaluateAll(ns=>ns.map(n=>n.id));check(new Set(ids).size===ids.length,'separate SVG marker IDs do not collide');
    await page.locator('#graph-find').fill('排队成员');await page.locator('#graph-find').press('Enter');
    check(await page.evaluate(()=>document.activeElement.dataset.graphLabel)==='排队成员 · 等待 ×3','keyboard search reaches named waiting node');
    check(await page.evaluate(()=>{const node=document.activeElement.getBoundingClientRect(),tools=document.querySelector('#graph-tools').getBoundingClientRect();return node.top>=tools.bottom && node.bottom<=innerHeight;}),'located node is not hidden behind sticky graph controls');
    await shot('replay-graph-located');
    check((await page.locator('#step-info').textContent()).includes('当前第 2 步'),'locating relation does not change replay step');
    await page.locator('#graph-fit').click();
    check(await page.locator('#mermaid-view svg text').evaluateAll(ns=>ns.every(n=>parseFloat(getComputedStyle(n).fontSize)*n.getScreenCTM().a>=12.9)),'fit width never makes already small graph labels smaller');
    check(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'grouped graph cannot widen page');
    await page.locator('#graph-item').selectOption('badge');await ready();
    await page.locator('#graph-actual').click();
    await page.locator('.replay-content').evaluate(n=>n.scrollTop=420);
    const scroll=await page.locator('.replay-content').evaluate(n=>n.scrollTop);
    await page.locator('#btn-prev').click();await ready();
    check(await page.locator('.replay-content').evaluate(n=>Math.abs(n.scrollTop-420)<2),'step changes preserve graph viewport');
    check(await page.evaluate(()=>window.graphCard===document.querySelector('.graph-card')),'step change retains keyed product card');
    const trace=[await page.locator('.replay-content').evaluate(n=>n.scrollTop)];
    await page.locator('#tab-table').click();trace.push(await page.locator('.replay-content').evaluate(n=>n.scrollTop));await page.locator('#tab-column').click();trace.push(await page.locator('.replay-content').evaluate(n=>n.scrollTop));await page.locator('#tab-mermaid').click();await ready();trace.push(await page.locator('.replay-content').evaluate(n=>n.scrollTop));
    check(await page.locator('#graph-item').inputValue()==='badge','view changes preserve product focus');
    check(await page.locator('#graph-find').inputValue()==='排队成员','view changes preserve locator query');
    check(Math.abs(await page.locator('.replay-content').evaluate(n=>n.scrollTop)-scroll)<2,'return to graph restores its own viewport: '+JSON.stringify({expected:scroll,trace}));
    // A slow old render may finish after a newer view; it must never replace it.
    await page.evaluate(()=>{const render=window.mermaid.render;window.mermaid.render=(id,def)=>new Promise(resolve=>setTimeout(()=>render(id,def).then(resolve),def.includes('立牌')?160:0));});
    await page.locator('#graph-item').selectOption('stand');await page.locator('#graph-item').selectOption('badge');await ready();
    await page.waitForTimeout(220);
    check(await page.locator('.graph-card h3 b').textContent()==='徽章','late old graph cannot overwrite current product');
    check((await page.locator('#step-info').textContent()).includes('当前第 1 步'),'all graph operations preserve current step');
    await page.locator('#tab-column').click();
    await page.locator('.colhead .column-step').nth(1).focus();await page.keyboard.press('Space');
    check((await page.locator('#step-info').textContent()).includes('当前第 2 步'),'native column button supports Space');
    check(await page.evaluate(()=>document.activeElement.classList.contains('column-step')),'column redraw restores keyboard focus');
    await page.locator('#tab-mermaid').click();await ready();
    check((await page.locator('#replay-notice').textContent()).includes('导入文件，完全只读'),'imported data does not keep the misleading active-round notice');
    for(const theme of ['dark','light']){
      await page.evaluate(async value=>{window.PAIGU_SHELL.theme.set(value);await new Promise(requestAnimationFrame);await Promise.all(document.getAnimations().filter(a=>Number.isFinite(a.effect.getComputedTiming().endTime)).map(a=>a.finished.catch(()=>{})));},theme);
      await page.locator('.replay-content').evaluate(n=>n.scrollTop=0);await shot('replay-product-graph-'+theme);
    }
    check(writes.length===0,'graph/import/locator makes no management writes');check(errors.length===0,'graph has no uncaught page errors: '+errors.join(';'));
  }catch(error){await shot('replay-graph-failure');throw error;}
  finally{page.off('pageerror',onError);page.off('request',onRequest);await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);}
}
