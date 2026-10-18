import fs from 'node:fs';
import path from 'node:path';

function check(v,m){if(!v)throw new Error(m);}
async function api(base,method,route,body){
  const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
  const value=await r.json();check(r.ok,method+' '+route+': '+r.status+' '+JSON.stringify(value).slice(0,160));return value;
}
export async function verifyScopedSettings(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'), errors=[], calls=[];
  const onError=e=>errors.push(String(e)), onRequest=r=>{if(r.method()!=='GET')calls.push(r.url());};
  page.on('pageerror',onError);page.on('request',onRequest);
  const capture=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});}};
  const guard=choice=>page.locator('.draft-guard [data-choice="'+choice+'"]').click();
  const save=async scope=>{
    const response=page.waitForResponse(r=>r.request().method()==='PUT'&&r.url().endsWith('/api/config'));
    await page.locator('#save-'+scope).click();check((await response).ok(),scope+' scoped save');
    await page.waitForFunction(s=>document.querySelector('#status-'+s).textContent.includes('本分区已保存'),scope);
  };
  try{
    await page.goto(`${ctx.base}/settings.html?api=${encodeURIComponent(ctx.base)}`);
    await page.locator('#form:not(.hidden)').waitFor();
    await page.evaluate(()=>PAIGU_SHELL.theme.set('light'));await page.waitForTimeout(200);
    check(await page.locator('#sec-gateway').isVisible()&&!await page.locator('#sec-llm').isVisible(),'one settings task visible, gateway default');
    const heartbeat=initial.config.gateway.heartbeat_secs+1, model=initial.config.llm.model+'-draft';
    await page.locator('#gw-heartbeat').fill(String(heartbeat));
    await page.locator('.section-nav a[href="#sec-llm"]').click();
    await page.locator('#llm-model').fill(model);
    await page.locator('.section-nav a[href="#sec-display"]').click();
    await page.locator('#dp-refresh').fill(String(initial.config.display.refresh_ms+500));
    await save('display');
    let current=await api(ctx.base,'GET','/api/config');
    check(current.config.gateway.heartbeat_secs===initial.config.gateway.heartbeat_secs,'display save does not commit gateway draft');
    check(current.config.llm.model===initial.config.llm.model,'display save does not commit understanding draft');
    check(JSON.stringify(current.config.members)===JSON.stringify(initial.config.members),'display save preserves member settings');
    await page.locator('.section-nav a[href="#sec-gateway"]').click();
    check(await page.locator('#gw-heartbeat').inputValue()===String(heartbeat),'tab switching retains the other draft');
    await save('gateway');current=await api(ctx.base,'GET','/api/config');
    check(current.config.gateway.heartbeat_secs===heartbeat&&current.config.gateway.reply_enabled===false,'gateway save remains no-reply');
    check(current.config.gateway.accept_forged_messages===initial.config.gateway.accept_forged_messages,'gateway save does not enable forged intake');
    // Another editor changed the same partition. Do not overwrite it or drop this draft.
    current.config.llm.max_tokens+=1;await api(ctx.base,'PUT','/api/config',{config:current.config,revision:current.revision});
    await page.locator('.section-nav a[href="#sec-llm"]').click();
    await page.locator('#save-understanding').click();
    await page.locator('#status-understanding').filter({hasText:'冲突'}).waitFor();
    check(await page.locator('#llm-model').inputValue()===model,'same-scope conflict keeps typed model name');
    check((await api(ctx.base,'GET','/api/config')).config.llm.model===initial.config.llm.model,'conflict did not write');
    await page.locator('#load').click();await guard('cancel');
    check(await page.locator('#llm-model').inputValue()===model,'cancel re-read retains draft and current section');
    await capture('settings-conflict-draft-light');
    const reloaded=page.waitForResponse(r=>r.request().method()==='GET'&&r.url().endsWith('/api/config'));
    await page.locator('#load').click();await guard('discard');await reloaded;
    await page.waitForFunction(()=>document.querySelector('#conn-badge').textContent==='已连接'&&!document.querySelector('#form').inert);
    await page.waitForFunction(expected=>document.querySelector('#llm-model').value===expected,initial.config.llm.model);
    await page.locator('#llm-model').fill(model);await save('understanding');
    check((await api(ctx.base,'GET','/api/config')).config.llm.model===model,'understanding explicitly saved: expected '+model+', got '+(await api(ctx.base,'GET','/api/config')).config.llm.model);
    // Real navigation offers all three choices, and save-and-continue has exact scope.
    await page.locator('#llm-model').fill(model+'-saved');
    await page.locator('#wf-stepper a[href*="members.html"]').click();
    await capture('settings-leave-guard-light');
    await page.locator('.draft-guard [data-choice="cancel"]').focus();await page.keyboard.press('Shift+Tab');
    check(await page.locator('.draft-guard [data-choice="save"]').evaluate(n=>n===document.activeElement),'native guard traps backward Tab');
    await page.keyboard.press('Escape');
    check(new URL(page.url()).pathname.endsWith('/settings.html'),'cancel navigation stays in workspace');
    check(await page.locator('#llm-model').inputValue()===model+'-saved','cancel navigation keeps text');
    await page.locator('#wf-stepper a[href*="members.html"]').click();await guard('save');
    await page.waitForFunction(()=>document.body?.dataset.page==='members');
    check((await api(ctx.base,'GET','/api/config')).config.llm.model===model+'-saved','save-and-continue persists before leaving');
    await page.locator('#wf-stepper a[href*="settings.html"]').click();
    await page.locator('#form:not(.hidden)').waitFor();
    await page.locator('#gw-heartbeat').fill(String(heartbeat+1));
    const alternate=ctx.base+'/other-instance';
    await page.locator('#api').fill(alternate);await page.locator('#api').press('Tab');await guard('cancel');
    check(await page.locator('#api').inputValue()===ctx.base,'cancel instance change restores current endpoint');
    check(await page.locator('#gw-heartbeat').inputValue()===String(heartbeat+1),'instance cancel keeps form draft');
    await page.locator('.section-nav a[href="#sec-snapshot"]').click();
    const beforeReload=calls.filter(x=>x.endsWith('/api/config/reload')).length;
    await page.locator('#reload').click();await guard('cancel');
    check(calls.filter(x=>x.endsWith('/api/config/reload')).length===beforeReload,'cancel disk maintenance produces no command');
    // Native browser reload/close also has a beforeunload safeguard.
    const unload=page.waitForEvent('dialog');const reload=page.reload().catch(()=>null);
    const prompt=await unload;check(prompt.type()==='beforeunload','browser reload warns about dirty state');await prompt.dismiss();await reload;
    check(await page.locator('#gw-heartbeat').inputValue()===String(heartbeat+1),'cancel browser reload keeps input');
    await page.locator('#wf-stepper a[href*="round.html"]').click();await guard('discard');
    await page.locator('#rd-category').waitFor();
    check(await page.locator('#rd-category').inputValue()==='test','round category is explicit');
    const originalCategory=await page.locator('#rd-category').inputValue();
    await page.locator('#rd-category').selectOption('normal');
    await page.locator('#wf-stepper a[href*="settings.html"]').click();await guard('cancel');
    check(await page.locator('#rd-category').inputValue()==='normal','round draft remains on canceled leave');
    await page.locator('#rd-category').selectOption(originalCategory);
    await page.locator('#rd-new-id').fill('unused-new-round');
    await page.locator('#wf-stepper a[href*="settings.html"]').click();await guard('discard');
    await page.locator('#form:not(.hidden)').waitFor();
    for(const theme of ['dark','light']){
      await page.evaluate(t=>PAIGU_SHELL.theme.set(t),theme);await page.locator('.section-nav a[href="#sec-llm"]').click();
      await page.waitForTimeout(200);check(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'settings has no page overflow');
      const geometry=await page.locator('#sec-llm').boundingBox();
      check(geometry.width>1000&&Math.abs(geometry.x+geometry.width/2-960)<2,'settings uses a centered task panel, not the discarded right-hand column');
      const first=await page.locator('label:has(#llm-enabled)').boundingBox(),second=await page.locator('label:has(#llm-fallback)').boundingBox();
      check(Math.abs(first.y-second.y)<1&&Math.abs(first.width-second.width)<1,'visual and DOM order use the same two equal columns');
      await capture('settings-understanding-'+theme);
    }
    check(!calls.some(x=>/\/suggest|\/cn-suggest|\/sim\/|\/rounds\/.*\/activate/.test(x)),'settings tasks never invoke a model, inject messages or switch rounds');
    check(errors.length===0,'no browser script errors: '+errors.join(';'));
  }finally{
    page.off('pageerror',onError);page.off('request',onRequest);
    const current=await api(ctx.base,'GET','/api/config');await api(ctx.base,'PUT','/api/config',{config:initial.config,revision:current.revision});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
