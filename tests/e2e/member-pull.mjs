import fs from 'node:fs';
import path from 'node:path';
function check(value,message){if(!value)throw new Error(message);}
async function get(base,route){const response=await fetch(base+route);check(response.ok,'read '+route);return response.json();}

export async function verifyMemberPullHealth(page,ctx){
  const calls=[],errors=[];
  const onRequest=request=>{if(request.method()!=='GET')calls.push(request);},onError=error=>errors.push(String(error));
  page.on('request',onRequest);page.on('pageerror',onError);
  const config=(await get(ctx.base,'/api/config')).config;
  const file=path.resolve(ctx.tmpDir,config.members.cache_path),relative=path.relative(ctx.tmpDir,file);
  check(relative&&!relative.startsWith('..')&&!path.isAbsolute(relative),'member fixture must stay inside this isolated server');
  const original=fs.existsSync(file)?fs.readFileSync(file):null;
  fs.mkdirSync(path.dirname(file),{recursive:true});
  fs.writeFileSync(file,JSON.stringify({group_id:config.members.group_id,members:[{user_id:'user01',card:'成员01'},{user_id:'user02',card:'成员02'}]}));
  const initial=await get(ctx.base,'/api/members');
  let displayed={running:false,running_for_current_source:false,schedule_state:'scheduled',next_attempt_at_ms:1900000000000,last_success_at_ms:1800000000000,
    consecutive_failures:3,last_attempt:{status:'failed',origin:'scheduled',reason:'receiver_error',started_at_ms:1800000100000,finished_at_ms:1800000101000,member_count:null}};
  let stale=false;
  const routeWorkflow=async route=>{const response=await route.fetch(),body=await response.json();body.members_pull=displayed;if(stale)body.revision+=1;await route.fulfill({response,json:body});};
  try{
    await page.route('**/api/workflow',routeWorkflow);
    await page.goto(`${ctx.base}/members.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#form:not(.hidden)').waitFor();await page.waitForFunction(()=>document.querySelector('#members-pull-status').textContent.includes('连续失败 3'));
    const name=page.locator('#members-body .cn').first();await name.fill('保留的称呼草稿');
    displayed={...displayed,running:true,running_for_current_source:true};await page.evaluate(()=>PAIGU_SHELL.refresh());
    await page.waitForFunction(()=>document.querySelector('#members-pull-status').textContent.includes('正在拉取成员'));
    check(await page.locator('#members-refresh').isDisabled(),'busy control prevents duplicate pulls');
    check(await name.inputValue()==='保留的称呼草稿','health polling cannot replace editable names');
    stale=true;await page.evaluate(()=>PAIGU_SHELL.refresh());await page.waitForFunction(()=>document.querySelector('#members-pull-status').textContent.includes('配置已变化'));
    check(await name.inputValue()==='保留的称呼草稿','stale status preserves drafts');
    stale=false;displayed={...displayed,running:false,running_for_current_source:false,schedule_state:'invalid_time',next_attempt_at_ms:null};
    await page.evaluate(()=>PAIGU_SHELL.refresh());await page.waitForFunction(()=>document.querySelector('#members-pull-status').textContent.includes('自动拉取暂停'));
    check(!await page.locator('#members-refresh').isDisabled(),'bad daily time does not prohibit an explicit guarded pull');
    for(const theme of ['light','dark']){
      await page.evaluate(value=>PAIGU_SHELL.theme.set(value),theme);await page.waitForTimeout(200);
      check(await page.evaluate(()=>innerWidth===1920&&innerHeight===1080&&devicePixelRatio===1&&document.documentElement.scrollWidth<=innerWidth),'member health desktop canvas and containment');
      if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,`members-pull-${theme}.png`)});}
    }
    check(calls.length===0,'status and theme changes send no model/pull/save commands');
    await page.locator('#members-refresh').click();await page.locator('.draft-guard [data-choice="cancel"]').click();
    check(calls.length===0,'cancelled draft guard never sends a pull');
    check(await name.inputValue()==='保留的称呼草稿','cancelled recovery keeps draft');
    await page.unroute('**/api/workflow',routeWorkflow);
    const response=page.waitForResponse(r=>r.url().endsWith('/api/members/refresh')&&r.request().method()==='POST');
    await page.locator('#members-refresh').click();await page.locator('.draft-guard [data-choice="discard"]').click();
    check((await response).status()===502,'real isolated API reports disconnected receiver');
    await page.waitForFunction(()=>document.querySelector('#members-pull-status').textContent.includes('连续失败'));
    const after=await get(ctx.base,'/api/members');
    check(after.members_fingerprint===initial.members_fingerprint,'failed recovery preserves actual cache/source fingerprint');
    check(after.pull_status.last_attempt.origin==='manual'&&after.pull_status.last_attempt.reason==='receiver_error','actual failed attempt is visible');
    check(calls.length===1&&calls[0].url().endsWith('/api/members/refresh'),'exactly one explicit pull, no model/config change');
    check(Object.keys(calls[0].postDataJSON()).sort().join(',')==='config_revision,members_fingerprint,round_id','recovery keeps exact source guards');
    check(errors.length===0,'no member health script errors: '+errors.join(';'));
  }finally{
    await page.unroute('**/api/workflow',routeWorkflow);page.off('request',onRequest);page.off('pageerror',onError);
    if(original)fs.writeFileSync(file,original);else fs.rmSync(file,{force:true});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
