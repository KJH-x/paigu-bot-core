import fs from 'node:fs';
import path from 'node:path';
function check(v,m){if(!v)throw new Error(m);}
async function api(base,method,route,body,status=200){const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const v=await r.json();check(r.status===status,method+' '+route+': '+r.status);return v;}
export async function verifyMemberBoundaries(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'), file=path.resolve(initial.config.members.cache_path);
  check(file.startsWith(path.resolve(ctx.tmpDir)+path.sep),'only an isolated member cache may be replaced');
  const original=fs.existsSync(file)?fs.readFileSync(file):null, errors=[], suggestions=[];
  const onError=e=>errors.push(String(e));page.on('pageerror',onError);
  const configRoute=async route=>{if(route.request().method()!=='GET')return route.continue();const r=await route.fetch();const v=await r.json();v.config.llm.enabled=true;await route.fulfill({response:r,json:v});};
  let release;
  const suggestRoute=async route=>{
    const c=route.request().postDataJSON();suggestions.push(c);
    if(suggestions.length===1)await new Promise(resolve=>{release=resolve;});
    await route.fulfill({json:{...c,source:'cache',executed:false,suggestions:[{user_id:'1',cn:'建议称呼甲'},{user_id:'2',cn:'建议称呼乙'}]}});
  };
  try{
    check(initial.config.llm.enabled===false,'real server model stays disabled');
    fs.writeFileSync(file,JSON.stringify({group_id:initial.config.round.group_id,members:[{user_id:1,card:'成员01 占位备注',nickname:'成员01'},{user_id:2,card:'成员02',nickname:'成员02'}]}));
    const view=await api(ctx.base,'GET','/api/members'), guard={round_id:view.round_id,config_revision:view.config_revision,members_fingerprint:view.members_fingerprint};
    check(view.members[0].user_id==='1'&&view.cached_group_id===initial.config.round.group_id,'numeric IDs and cache provenance are normalized');
    await api(ctx.base,'POST','/api/members/cn-suggest',{},400);
    await api(ctx.base,'POST','/api/members/refresh',{},400);
    await api(ctx.base,'POST','/api/members/cn-suggest',guard,422);
    await api(ctx.base,'POST','/api/members/refresh',{...guard,members_fingerprint:'stale'},409);
    // Browser-only capability and model mock; remove the config mock before any save.
    await page.route(ctx.base+'/api/config',configRoute);await page.route(ctx.base+'/api/members/cn-suggest',suggestRoute);
    await page.goto(`${ctx.base}/members.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#members-body .cn').nth(1).fill('人工称呼乙');
    await page.locator('#members-cn-llm').click();
    for(let i=0;i<100&&!release;i++)await new Promise(resolve=>setTimeout(resolve,20));check(release,'explicit model mock began');
    await page.locator('#members-body .cn').first().fill('请求期间人工甲');release();
    await page.locator('#members-ai-info').filter({hasText:'建议 2 条'}).waitFor();
    check(!(await page.locator('#members-ai-body input').nth(1).isChecked()),'custom CN is not selected by default');
    await page.locator('#members-ai-apply').click();
    check(await page.locator('#members-body .cn').first().inputValue()==='请求期间人工甲','delayed model preview never overwrites a manual edit');
    check(await page.locator('#members-body .cn').nth(1).inputValue()==='人工称呼乙','custom CN remains untouched');
    await page.locator('#members-cn-llm').click();await page.locator('#members-ai-info').filter({hasText:'建议 2 条'}).waitFor();
    await page.locator('#members-ai-body input').first().check();await page.locator('#members-ai-apply').click();
    check(await page.locator('#members-body .cn').first().inputValue()==='建议称呼甲','explicit selection fills only the chosen draft');
    check((await api(ctx.base,'GET','/api/config')).config.members.cn_overrides.length===initial.config.members.cn_overrides.length,'preview and application do not save');
    check(suggestions.length===2&&suggestions.every(c=>Object.keys(c).sort().join(',')==='config_revision,members_fingerprint,round_id'),'only explicit requests carry exact guards, not a full browser name dump');
    await page.unroute(ctx.base+'/api/config',configRoute);
    let response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/config'));
    await page.locator('#members-cn-save').click();check((await response).ok(),'explicit CN save succeeds');await page.waitForFunction(()=>!document.querySelector('#members-cn-save').disabled);
    const saved=await api(ctx.base,'GET','/api/config');
    check(saved.config.members.cn_overrides.find(m=>m.user_id==='1').cn==='建议称呼甲'&&saved.config.members.cn_overrides.find(m=>m.user_id==='2').cn==='人工称呼乙','chosen and custom CN persist independently');
    check(saved.config.llm.enabled===false,'browser model mock never enables the actual model');
    const before=fs.readFileSync(file);
    response=page.waitForResponse(r=>r.request().method()==='POST'&&r.url().endsWith('/api/members/refresh'));
    await page.locator('#members-refresh').click();check((await response).status()===502,'disconnected receiver reports an upstream failure');
    await page.locator('#banner').filter({hasText:'拉取失败'}).waitFor();
    check(fs.readFileSync(file).equals(before),'failed pull never replaces the cache');
    check(await page.locator('#members-body .cn').first().inputValue()==='建议称呼甲','failed pull leaves visible CN intact');
    if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,'members-disconnected-pull.png')});}
    check(errors.length===0,'member page errors: '+errors.join(';'));
  }finally{
    await page.unroute(ctx.base+'/api/config',configRoute);await page.unroute(ctx.base+'/api/members/cn-suggest',suggestRoute);page.off('pageerror',onError);
    const current=await api(ctx.base,'GET','/api/config');await api(ctx.base,'PUT','/api/config',{config:initial.config,revision:current.revision});
    if(original)fs.writeFileSync(file,original);else fs.rmSync(file,{force:true});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
