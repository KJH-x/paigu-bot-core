import path from 'node:path';
function check(v,m){if(!v)throw new Error(m);}
async function api(base,method,route,body){
  const r=await fetch(base+route,{method,headers:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
  const data=await r.json();check(r.ok,`${method} ${route}: ${r.status}`);return data;
}
export async function verifyCatalogReachability(page,ctx){
  const initial=await api(ctx.base,'GET','/api/config'),round='e2e-catalog-scroll',kinds=['group','single','box','gift'];
  const items=kinds.flatMap((kind,k)=>Array.from({length:3},(_,i)=>({
    item_id:`item-${k}-${i}`,name:`占位商品${k}${i}`,kind,aliases:[],unit_price_cents:1000,
    subitems:['group','gift'].includes(kind)?Array.from({length:8},(_,j)=>({subitem_id:`s-${j}`,name:'款式'+j,capacity:2,aliases:[],unit_price_cents:1000,adjust_cents:0})):[]
  })));
  try{
    await api(ctx.base,'POST','/api/rounds',{round_id:round,title:'目录滚动验收',category:'test',copy_from:initial.config.round.round_id});
    await api(ctx.base,'POST',`/api/rounds/${round}/setup`,{items,phases:[],priority_users:[],priority_window:null});
    await api(ctx.base,'POST',`/api/rounds/${round}/activate`,{mode:'continue'});
    const revision=(await api(ctx.base,'GET','/api/config')).revision;
    await page.goto(`${ctx.base}/round.html?api=${encodeURIComponent(ctx.base)}#sec-items`);
    await page.locator('#kind-tabs button').last().waitFor();
    for(const theme of ['light','dark']){
      await page.evaluate(async t=>{PAIGU_SHELL.theme.set(t);await new Promise(requestAnimationFrame);await Promise.all(document.getAnimations().filter(a=>Number.isFinite(a.effect.getComputedTiming().endTime)).map(a=>a.finished.catch(()=>{})));},theme);
      for(const [k,kind] of kinds.entries()){
        await page.locator(`#kind-tabs [data-kind="${kind}"]`).click();
        const names=page.locator('#items input[data-scope="item"][data-f="name"]');
        for(let i=0;i<3;i++){
          const name=names.nth(i);check(await name.inputValue()===`占位商品${k}${i}`,'category displays the correct original catalog');
          await name.evaluate(n=>n.scrollIntoView({block:'center',behavior:'instant'}));await name.click();
          check(await name.evaluate(n=>{const b=n.getBoundingClientRect();return n===document.activeElement&&document.elementFromPoint(b.x+b.width/2,b.y+b.height/2)===n;}),'first/middle/last name input remains clickable and unoccluded');
          check(await page.locator('#kind-tabs').evaluate(n=>{const b=n.getBoundingClientRect();return b.y>=0&&b.bottom<innerHeight;}),'category navigation stays reachable during inner scroll');
          check(await page.locator('#save').isVisible(),'global save remains reachable');
        }
        if(kind==='group'){
          const scroll=await page.locator('#sec-items > .panel-body').evaluate(n=>({top:n.scrollTop,overflow:n.scrollHeight>n.clientHeight}));
          check(scroll.overflow&&scroll.top>0,'long catalog actually exercised inner vertical scrolling');
          await page.locator('#item-filter').fill(`占位商品${k}1`);
          check(await names.count()===1&&await names.first().inputValue()===`占位商品${k}1`,'catalog filtering retains the matching product');
          await page.locator('#item-filter').fill('');check(await names.count()===3,'clearing filter restores all real products');
          if(process.env.PAIGU_UI_CAPTURE_DIR)await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,'catalog-scroll-'+theme+'.png')});
        }
        check(await page.evaluate(()=>innerWidth===1920&&innerHeight===1080&&devicePixelRatio===1&&document.documentElement.scrollWidth<=innerWidth),'desktop canvas and no page horizontal overflow');
      }
    }
    check((await api(ctx.base,'GET','/api/config')).revision===revision,'scroll, filter, focus and theme never save or mutate catalog');
  }finally{
    await api(ctx.base,'POST',`/api/rounds/${encodeURIComponent(initial.config.round.round_id)}/activate`,{mode:'continue'});
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
