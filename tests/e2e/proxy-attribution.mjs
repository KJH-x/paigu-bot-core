import fs from 'node:fs';
import path from 'node:path';

function check(value,message){if(!value)throw new Error(message);}
async function api(base,method,route,body){
  const response=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
  check(response.ok,method+' '+route+': '+response.status);return response.json();
}

export async function verifyProxyAttribution(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-proxy-attribution',errors=[];
  const onError=error=>errors.push(String(error));page.on('pageerror',onError);
  const file=path.resolve(ctx.tmpDir,initial.config.members.cache_path),relative=path.relative(ctx.tmpDir,file);
  check(relative&&!relative.startsWith('..')&&!path.isAbsolute(relative),'proxy fixture stays inside isolated server');
  const old=fs.existsSync(file)?fs.readFileSync(file):null;
  try{
    check(!initial.config.llm.enabled&&!initial.config.gateway.reply_enabled,'no model or group sending');
    fs.mkdirSync(path.dirname(file),{recursive:true});
    fs.writeFileSync(file,JSON.stringify({group_id:initial.config.round.group_id,members:[{user_id:'proxy-member',card:'成员乙'}]}));
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'代排归属验收',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{phases:[],priority_users:[],priority_window:null,
      items:[{item_id:'badge',name:'徽章',kind:'拼团',aliases:[],unit_price_cents:1000,
        subitems:[{subitem_id:'a',name:'甲',aliases:[],capacity:1,unit_price_cents:1000,adjust_cents:0}]}]});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'fresh'});
    for(const [i,text] of ['代成员乙排:排 徽章 甲 1','代1排:排 徽章 甲 1'].entries()){
      await api(ctx.base,'POST','/api/sim/messages',[{user_id:'proxy-actor',nickname:'发言甲',group_id:initial.config.round.group_id,text,timestamp:Date.now()+i}]);
    }
    await api(ctx.base,'POST','/api/sim/messages',[{user_id:'another-actor',nickname:'发言甲',group_id:initial.config.round.group_id,text:'代1排:排 徽章 甲 1',timestamp:Date.now()+3}]);
    const records=(await api(ctx.base,'GET','/api/messages')).messages;
    check(records.length===3&&records[0].user_id==='proxy-member'&&records[1].user_id.startsWith('proxy:')&&records[1].user_id!==records[2].user_id,'real API freezes distinct owners even for same-named actors');
    check(records[0].delegation.actor_user_id==='proxy-actor','real sender is not overwritten');
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#messages .msg').nth(1).waitFor();
    await page.waitForFunction(()=>document.querySelectorAll('#who-whats > tr').length===3);
    check((await page.locator('#who-whats').textContent()).split('发言甲(代1)').length===3,'same display names keep two real summary rows');
    const dialog=page.getByRole('dialog',{name:'修订消息动作'});
    for(const [i,source] of ['代成员乙排:排 徽章 甲 1','代1排:排 徽章 甲 1'].entries()){
      await page.locator('#messages .msg').nth(i).click();
      await dialog.locator('.ae-proxy-source').waitFor();
      await dialog.locator('.ae-proxy-source summary').click();
      check((await dialog.locator('.ae-proxy-source').textContent()).includes('发言甲'),'source shows actual actor');
      check((await dialog.locator('.ae-proxy-source .ae-raw').textContent())===source,'full source text is readable');
      check((await dialog.locator('.ae-proxy-source .hint').textContent()).includes(i===0?'被代排人':'原发言人'),'qualification explanation follows the owner type');
      for(const theme of ['light','dark']){
        await page.evaluate(value=>PAIGU_SHELL.theme.set(value),theme);await page.waitForTimeout(200);
        check(await page.evaluate(()=>innerWidth===1920&&innerHeight===1080&&devicePixelRatio===1&&document.documentElement.scrollWidth<=innerWidth),'1920x1080 at 100% without page overflow');
        const sourceBox=await dialog.locator('.ae-proxy-source').boundingBox();
        check(sourceBox&&sourceBox.x>=0&&sourceBox.x+sourceBox.width<=1920,'source panel remains contained');
        if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,`proxy-${i===0?'member':'guest'}-${theme}.png`)});}
      }
      await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});
    }
    check(JSON.stringify(records.map(r=>[r.record_id,r.record_revision,r.text,r.delegation]))===JSON.stringify((await api(ctx.base,'GET','/api/messages')).messages.map(r=>[r.record_id,r.record_revision,r.text,r.delegation])),'viewing source and themes never changes records');
    check(!errors.length,errors.join('; '));
  }finally{
    page.off('pageerror',onError);
    if(old===null)fs.rmSync(file,{force:true});else fs.writeFileSync(file,old);
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'fresh'});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
