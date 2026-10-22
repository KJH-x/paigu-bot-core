import fs from 'node:fs';
import path from 'node:path';

function check(value,message){if(!value)throw new Error(message);}
async function api(base,method,route,body){
  const response=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
  check(response.ok,method+' '+route+': '+response.status);return response.json();
}

export async function verifyTargetModify(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-target-modify',errors=[];
  const onError=error=>errors.push(String(error));page.on('pageerror',onError);
  const file=path.resolve(ctx.tmpDir,initial.config.members.cache_path),relative=path.relative(ctx.tmpDir,file);
  check(relative&&!relative.startsWith('..')&&!path.isAbsolute(relative),'target fixture stays in this isolated server');
  const previous=fs.existsSync(file)?fs.readFileSync(file):null;
  try{
    check(!initial.config.llm.enabled&&!initial.config.gateway.reply_enabled,'no model or real group sending');
    fs.mkdirSync(path.dirname(file),{recursive:true});
    fs.writeFileSync(file,JSON.stringify({group_id:initial.config.round.group_id,members:[{user_id:'target-member',card:'成员乙'}]}));
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'管理员目标改单验收',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{phases:[],priority_users:[],priority_window:null,
      items:[{item_id:'badge',name:'徽章',kind:'拼团',aliases:[],unit_price_cents:1000,
        subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]}]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    let cfg=await api(ctx.base,'GET','/api/config');
    await api(ctx.base,'POST','/api/config/admin-commands',{enabled:true,revision:cfg.revision});
    const send=(user_id,nickname,text,is_admin=false)=>api(ctx.base,'POST','/api/sim/messages',
      [{user_id,nickname,group_id:initial.config.round.group_id,text,is_admin,timestamp:Date.now()}]);
    await send('target-member','成员乙','排 徽章 甲 1');
    const command='/改单 成员乙 徽章 甲 3';
    await send('admin-actor','管理甲',command,true);
    const records=(await api(ctx.base,'GET','/api/messages')).messages;
    check(records.length===2&&records[1].text===command&&records[1].user_id==='target-member','one original-position command, selected owner, unchanged raw text');
    check(records[1].delegation.actor_user_id==='admin-actor'&&records[1].delegation.target.kind==='admin_modify','real admin actor is separately preserved');
    const board=(await api(ctx.base,'GET','/api/board')).board;
    check(board.user_summaries.length===1&&board.user_summaries[0].items[0].quantity===3,'replace one with three, not four and not admin-owned');
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    const row=page.locator('#messages .msg').filter({hasText:command});
    await row.waitFor();await row.focus();await page.keyboard.press('Enter');
    const dialog=page.getByRole('dialog',{name:'修订消息动作'});
    await dialog.locator('.ov-qty').waitFor();
    check(await dialog.getByRole('combobox',{name:'动作类型',exact:true}).inputValue()==='modify','modify semantics prefill the existing editor');
    check(await dialog.locator('.ov-qty').inputValue()==='3','current quantity is prefilled');
    check(await dialog.locator('.ae-original > .ae-raw').textContent()===command,'original command is visible exactly once');
    const source=dialog.locator('.ae-proxy-source summary');await source.focus();await page.keyboard.press('Enter');
    check(await dialog.locator('.ae-proxy-source').evaluate(element=>element.open),'keyboard actually expands the source panel');
    check((await source.textContent()).includes('管理员改单来源 · 管理甲'),'source identifies actual admin');
    check((await dialog.locator('.ae-proxy-source .hint').textContent()).includes('阶段与锁定限制'),'permission explanation does not promise exemption');
    check(await dialog.locator('.ae-proxy-source .ae-raw').count()===0,'identical raw command is not repeated under source');
    for(const theme of ['light','dark']){
      await page.evaluate(value=>PAIGU_SHELL.theme.set(value),theme);await page.waitForTimeout(200);
      await dialog.locator('.ov-ok').scrollIntoViewIfNeeded();await source.scrollIntoViewIfNeeded();
      check(await page.evaluate(()=>innerWidth===1920&&innerHeight===1080&&devicePixelRatio===1&&document.documentElement.scrollWidth<=innerWidth),'desktop 1920x1080 at 100%, no horizontal page overflow');
      if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,`target-modify-${theme}.png`)});}
    }
    await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});
    check(await row.evaluate(element=>element===document.activeElement),'closing returns keyboard focus to the original row');
    check(JSON.stringify((await api(ctx.base,'GET','/api/messages')).messages)===JSON.stringify(records),'read, theme, scroll and close do not save or rewrite records');
    cfg=await api(ctx.base,'GET','/api/config');await api(ctx.base,'POST','/api/config/admin-commands',{enabled:false,revision:cfg.revision});
    const after=(await api(ctx.base,'GET','/api/board')).board;
    check(after.user_summaries[0].items[0].quantity===3,'turning execution off does not undo saved history');
    await send('admin-actor','管理甲','/改单 成员乙 徽章 甲 5',true);
    check((await api(ctx.base,'GET','/api/board')).board.user_summaries[0].items[0].quantity===3,'disabled future command records but does not execute');
    check(!errors.length,errors.join('; '));
  }finally{
    page.off('pageerror',onError);
    if(previous===null)fs.rmSync(file,{force:true});else fs.writeFileSync(file,previous);
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'fresh'});
    const cfg=await api(ctx.base,'GET','/api/config');
    await api(ctx.base,'POST','/api/config/admin-commands',{enabled:initial.config.gateway.admin_commands_enabled,revision:cfg.revision});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
