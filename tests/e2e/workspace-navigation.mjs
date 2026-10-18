import fs from 'node:fs';
import path from 'node:path';

function check(value,message){if(!value)throw new Error(message);}
export async function verifyWorkspaceNavigation(page,ctx){
  const keys=['index','round','display','settlement','replay','members','settings'];
  const labels=['总览','轮次与商品','排谷','结算与下单','复盘','成员','系统设置'];
  const initial=await fetch(ctx.base+'/api/config').then(r=>r.json());
  const errors=[], foreign=[];
  const onError=e=>errors.push(String(e));page.on('pageerror',onError);
  const guard=async route=>{
    const url=route.request().url();
    if(url.startsWith(ctx.base+'/') || url.startsWith(ctx.remote.base+'/'))return route.continue();
    foreign.push(url);return route.abort();
  };
  await page.route(/^https?:/,guard);
  const capture=async name=>{if(process.env.PAIGU_UI_CAPTURE_DIR){fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});}};
  try{
    await page.evaluate(()=>localStorage.setItem('paigu.display.api','http://127.0.0.1:1'));
    let visits=0;
    for(const source of keys){
      await page.goto(`${ctx.base}/${source}.html?api=${encodeURIComponent(ctx.base)}`);
      await page.locator('#wf-stepper a').last().waitFor();
      check(JSON.stringify(await page.locator('#wf-stepper .lbl').allTextContents())===JSON.stringify(labels),source+' has the same seven workspace labels');
      await page.evaluate(()=>document.body.dataset.navigationMarker='preserved');
      await page.locator(`#wf-stepper a[href*="/${source}.html"]`).click();
      check(await page.evaluate(()=>document.body.dataset.navigationMarker)==='preserved',source+' self navigation does not reload or discard state');
      for(const target of keys.filter(k=>k!==source)){
        await page.locator(`#wf-stepper a[href*="/${target}.html"]`).click();
        await page.waitForFunction(key=>document.body?.dataset.page===key,target);
        await page.locator('#workspace-title').waitFor();
        check(new URL(page.url()).searchParams.get('api')===ctx.base,source+' → '+target+' keeps API');
        check(await page.locator('#api').inputValue()===ctx.base,'header API matches body source');
        check((await page.locator('#wf-stepper [aria-current="page"]').getAttribute('href')).includes('/'+target+'.html'),'selected workspace matches destination');
        check(await page.locator('main').count()===1 && await page.locator('h1').count()===1,target+' has one main and one primary heading');
        check(await page.evaluate(()=>innerWidth===1920&&innerHeight===1080&&devicePixelRatio===1&&document.documentElement.scrollWidth<=innerWidth),target+' desktop does not overflow');
        visits++;
        await page.goBack();await page.waitForFunction(key=>document.body?.dataset.page===key,source);
      }
    }
    check(visits===42,'all 42 actual cross-workspace clicks completed');
    for(const route of ['/admin','/admin.html','/web/admin.html']){
      await page.goto(`${ctx.base}${route}?api=${encodeURIComponent(ctx.base)}#legacy-anchor`);
      await page.waitForFunction(()=>document.body?.dataset.page==='index');
      check(new URL(page.url()).hash==='#legacy-anchor' && new URL(page.url()).searchParams.get('api')===ctx.base,'admin compatibility preserves query and hash');
    }
    for(const key of keys){
      for(const route of ['/'+key,'/web/'+key+'.html']){
        await page.goto(`${ctx.base}${route}?api=${encodeURIComponent(ctx.base)}`);
        await page.waitForFunction(expected=>document.body?.dataset.page===expected,key);
        check(await page.locator('#wf-stepper a').count()===7,route+' exposes all workspaces');
      }
    }
    await page.goto(ctx.base+'/settings');
    await page.waitForFunction(()=>document.querySelector('#wf-api')?.textContent.includes('正常'));
    check(await page.locator('#api').inputValue()===ctx.base,'same-origin deployment ignores an unrelated stale stored API');
    check(await page.locator('#members-body,#msg-body').count()===0,'settings no longer owns members or message editing');
    await page.goto(`${ctx.base}/settlement.html?api=${encodeURIComponent(ctx.base)}#order`);
    await page.waitForFunction(()=>document.querySelector('.section-nav a[aria-current="location"]')?.hash==='#order');
    check((await page.locator('#wf-stepper [aria-current="page"]').textContent()).includes('结算与下单'),'order anchor stays in its single workspace');
    await page.goto(`${ctx.base}/settings.html?api=${encodeURIComponent(ctx.base)}#sec-messages`);
    await page.waitForFunction(()=>document.body?.dataset.page==='display'&&!!document.querySelector('#pf-msg-body'));
    check(await page.locator('#sec-msglog input').count()===0,'legacy message records are read-only and reuse action editor');
    await page.goto(`${ctx.base}/members.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(()=>document.querySelector('#wf-clients')?.textContent.includes('未连接接收器'));
    check(!(await page.locator('#wf-clients').getAttribute('class')).split(' ').includes('ok'),'zero receivers never appears green');
    for(const theme of ['light','dark']){
      await page.evaluate(t=>PAIGU_SHELL.theme.set(t),theme);
      await page.waitForTimeout(200);
      const contrast=await page.evaluate(()=>{
        function rgb(s){return s.match(/[\d.]+/g).slice(0,3).map(Number);}
        function lum(c){return c.map(x=>{x/=255;return x<=.04045?x/12.92:((x+.055)/1.055)**2.4;}).reduce((v,x,i)=>v+x*[.2126,.7152,.0722][i],0);}
        function ratio(el){const style=getComputedStyle(el);let bg=style.backgroundColor,n=el;while(bg==='rgba(0, 0, 0, 0)'&&n.parentElement){n=n.parentElement;bg=getComputedStyle(n).backgroundColor;}const a=lum(rgb(style.color)),b=lum(rgb(bg));return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);}
        return {heading:ratio(document.querySelector('#workspace-title')),api:ratio(document.querySelector('#api')),name:ratio(document.querySelector('.cn'))};
      });
      check(contrast.heading>=4.5 && contrast.api>=4.5 && contrast.name>=3,theme+' heading/input contrast is readable: '+JSON.stringify(contrast));
      await capture('workspace-members-'+theme);
    }
    check(foreign.length===0,'no navigation requested another API instance: '+foreign.join(','));
    check(errors.length===0,'workspace scripts have no uncaught errors: '+errors.join(';'));
    const after=await fetch(ctx.base+'/api/config').then(r=>r.json());
    check(after.config.round.round_id===initial.config.round.round_id && after.revision===initial.revision,'navigation never switches rounds or saves configuration');
  }finally{
    page.off('pageerror',onError);await page.unroute(/^https?:/,guard);
    await page.evaluate(()=>localStorage.removeItem('paigu.display.api'));
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
