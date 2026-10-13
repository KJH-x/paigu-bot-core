import fs from 'node:fs';
import path from 'node:path';

function check(value, message) { if (!value) throw new Error(message); }
const longName = '成员姓名特别长ABC';
const items = [
  {item_id:'board_fixture',name:'固定宽度验收 · 多盒拼团',kind:'拼团',variants:[
    {variant_id:'a',name:'款式甲',capacity:2}, {variant_id:'b',name:'款式乙',capacity:3}]},
  {item_id:'overflow_fixture',name:'固定宽度验收 · 超宽排表',kind:'拼团',variants:[{variant_id:'c',name:'款式丙',capacity:20}]}
];
const slot = (index, user='', status='filled', extra={}) => ({slot_index:index,user,status:user?status:'empty',...extra});
function board(long) {
  return {round_id:'width-fixture',item_allocations:[
    {item_id:'board_fixture',variant_id:'a',singles:[{display:'单领姓名成员ABC',quantity:3}],waiting:[{display:'等待姓名成员DEF',quantity:2}],boxes:[
      {box_index:1,slots:[slot(1,long?longName:'成员甲'),slot(2,'',null,{status:'locked_empty',policy:'tail',segment_id:'tail-fixture'})]},
      {box_index:2,slots:[slot(1,'短二'),slot(2,'Alpha')]}]},
    {item_id:'board_fixture',variant_id:'b',boxes:[
      {box_index:1,slots:[slot(1,'成员02'),slot(2,'成员乙', 'locked'),slot(3,'')]},
      {box_index:2,slots:[slot(1,'成员乙乙','admin_reserved'),slot(2,'中英AB'),slot(3,'')]}]},
    {item_id:'overflow_fixture',variant_id:'c',boxes:[{box_index:1,slots:Array.from({length:20},(_,i)=>slot(i+1,'超长姓名成员'+String(i+1).padStart(2,'0')+'ABC'))}]}]
  };
}

export async function verifyBoardLayout(page, ctx) {
  const original = await fetch(ctx.base+'/api/config').then(r=>r.json());
  const config = structuredClone(original);
  config.config.round = {...config.config.round,round_id:'width-fixture',title:'排表固定宽度验收',items};
  let liveBoard = board(true), liveVersion = 3;
  const history = {round_id:'width-fixture',title:'排表固定宽度验收',source:'active_round',version:3,items,
    messages:[{seq:1,display:'成员甲',text:'首次排入',status:'Applied'},{seq:2,display:longName,text:'改为全长姓名',status:'Applied'}],
    steps:[{index:0,message_seq:1,board:board(false)},{index:1,message_seq:2,board:board(true)}],board:board(true)};
  const workflow = {round_id:'width-fixture',title:history.title,revision:config.revision,version:3,messages:2,
    gateway:{clients:0,listening:true},locked:false};
  const fulfill = (route, body) => route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(body)});
  const routes = [
    ['**/api/config', route=>fulfill(route,config)],
    ['**/api/display?*', route=>fulfill(route,{version:liveVersion,board:liveBoard,messages:[],who_whats:[],status:{}})],
    ['**/api/replay/view', route=>fulfill(route,history)],
    ['**/api/workflow', route=>fulfill(route,workflow)]
  ];
  for (const [url, handler] of routes) await page.route(url,handler);
  const errors = [];
  const onError = error => errors.push(error.message);
  page.on('pageerror',onError);
  async function capture(name) {
    if (!process.env.PAIGU_UI_CAPTURE_DIR) return;
    fs.mkdirSync(process.env.PAIGU_UI_CAPTURE_DIR,{recursive:true});
    await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name+'.png')});
    if (name==='board-compact-light'||name==='board-full-light') {
      const panel=await page.locator('[data-col="board"]').boundingBox();
      const section=await page.locator('#board .item-section').first().boundingBox();
      await page.screenshot({path:path.join(process.env.PAIGU_UI_CAPTURE_DIR,name.replace('-light','-detail')+'.png'),
        clip:{x:panel.x,y:panel.y,width:panel.width,height:section.y+section.height-panel.y}});
    }
  }
  async function metrics(host) {
    return page.locator(host).evaluate(root=>{
      const sections=[...root.querySelectorAll('.item-section')];
      const cells=[...sections[0].querySelectorAll('.cell')];
      const rows=[...sections[0].querySelectorAll('.vrow')];
      return {widths:cells.map(c=>c.getBoundingClientRect().width),
        aligned:rows.map(row=>[...row.querySelectorAll('.cell')].map(c=>({box:c.dataset.box,index:c.__data.index,x:c.getBoundingClientRect().left}))),
        clipped:[...root.querySelectorAll('.cell-user, .chip-user')].some(c=>c.scrollWidth>c.clientWidth+1),
        overflow:document.documentElement.scrollWidth>innerWidth,
        innerOverflow:sections[1].querySelector('.rows').scrollWidth>sections[1].querySelector('.rows').clientWidth,
        hint:cells[0].title,accessible:cells[0].getAttribute('aria-label'),
        marks:cells.map(c=>({status:!c.__status.hidden,tail:!c.__tail.hidden,
          top:c.querySelector('.cell-box-mark').getBoundingClientRect().height,bottom:c.querySelector('.cell-marks').getBoundingClientRect().height}))};
    });
  }
  function aligned(m) {
    const first=m.aligned[0], second=m.aligned[1];
    first.forEach(c=>{
      const same=second.find(other=>other.box===c.box && other.index===c.index);
      check(same && Math.abs(same.x-c.x)<=1,'对应盒位应左对齐');
    });
  }
  try {
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForSelector('.board-grid .cell');
    await page.evaluate(()=>document.fonts.ready);
    check(await page.locator('[data-width-mode="compact"]').getAttribute('aria-pressed')==='true','首次打开默认三字宽');
    await page.click('[data-width-mode="compact"]');
    const compact=await metrics('#board');
    check(compact.widths.every(w=>Math.abs(w-compact.widths[0])<1),'空位和占用格必须三字等宽');
    const measured=await page.evaluate(()=>window.PAIGU.boardLayout.measure(document.querySelector('#board'),'汉汉汉')+6);
    check(Math.abs(compact.widths[0]-measured)<1,'姓名区域必须按实际字体三字测量');
    check(compact.clipped,'三字模式裁切长姓名');
    const queueWidth=await page.locator('#board .chip').first().evaluate(node=>node.getBoundingClientRect().width);
    check(Math.abs(queueWidth-compact.widths[0])<1,'单领/等待姓名也必须三字等宽');
    check((await page.locator('#board .chip-qty').allTextContents()).includes('×3'),'数量必须展示在姓名下方');
    check(!compact.overflow,'三字模式不撑宽页面');
    check(compact.hint.includes(longName)&&compact.accessible===compact.hint,'完整姓名应保留在提示和无障碍标签');
    check(compact.marks.some(m=>m.status&&m.tail),'包尾锁定标记应同时保留');
    check(compact.marks.every(m=>m.top===2&&m.bottom===4),'附加信息只能占上下标记区');
    aligned(compact);
    await capture('board-compact-light');
    await page.evaluate(()=>{window.testCell=document.querySelector('#board .cell');});
    await page.click('[data-width-mode="full"]');
    const full=await metrics('#board');
    check(!full.clipped,'全长姓名不能裁切');
    check(!full.overflow&&full.innerOverflow,'超宽排表只允许内部横向滚动');
    check(full.widths[0]>full.widths[2],'不同盒列不应被同一个长名字撑宽');
    aligned(full);
    check(await page.evaluate(()=>window.testCell===document.querySelector('#board .cell')),'切换不能重建格子');
    await capture('board-full-light');
    await page.click('#theme-toggle');
    await capture('board-full-dark');
    await page.click('[data-width-mode="compact"]');
    await capture('board-compact-dark');
    await page.click('#theme-toggle');
    liveBoard=board(true);
    liveBoard.item_allocations[0].boxes[0].slots[0].user='改名成员';
    liveVersion++;
    await page.click('#refresh');
    await page.waitForFunction(()=>document.querySelector('#board .cell-user').textContent==='改名成员');
    check(await page.evaluate(()=>window.testCell===document.querySelector('#board .cell')),'增量更新不能重建格子');
    const badge=page.locator('#board .cell .badge').first();
    check(await badge.isVisible(),'变化标记应显示');
    await page.click('[data-width-mode="full"]');
    check(await badge.isVisible(),'切换宽度不应清掉变化标记');
    await page.waitForFunction(()=>document.querySelector('#board .cell .badge').hidden,{timeout:5000});
    check((await page.locator('#board .cell').first().getAttribute('title')).includes('改名成员'),'高亮到期不能清掉姓名提示');
    await page.goto(`${ctx.base}/replay.html?api=${encodeURIComponent(ctx.base)}`);
    await page.waitForFunction(()=>document.querySelector('#step-info').textContent.includes('当前第 2 步'));
    check((await page.locator('#step-list .seq').allTextContents()).join(',')==='#1,#2','每条复盘消息必须有独立渲染身份');
    check(await page.locator('[data-width-mode="full"]').getAttribute('aria-pressed')==='true','宽度选择必须跨页保留');
    const end=await metrics('#table-view');
    await page.click('#btn-first');
    const start=await metrics('#table-view');
    check(JSON.stringify(start.widths)===JSON.stringify(end.widths),'复盘按全历史定宽，切步不能跳列');
    await page.click('[data-width-mode="compact"]');
    check((await page.textContent('#step-info')).includes('当前第 1 步'),'宽度切换不能重置当前步');
    await capture('replay-compact');
    await page.click('[data-width-mode="full"]');
    await capture('replay-full');
    await page.locator('#table-view .cell').first().click();
    check((await page.textContent('#step-info')).includes('当前第 2 步'),'点击格子仍应追溯最近修改');
    await page.click('#tab-column');
    const clipped=await page.locator('.col-table .cell-user, .col-name').evaluateAll(nodes=>nodes.filter(n=>n.scrollWidth>n.clientWidth+1).map(n=>({text:n.textContent,width:n.clientWidth,scroll:n.scrollWidth,font:getComputedStyle(n).font,spacing:getComputedStyle(n).letterSpacing})));
    await capture('replay-column-full');
    check(!clipped.length,'列视图全长姓名不能裁切：'+JSON.stringify(clipped));
    await page.click('[data-width-mode="compact"]');
    const widths=await page.locator('.colhead').evaluateAll(nodes=>nodes.map(n=>n.getBoundingClientRect().width));
    check(widths.every(w=>Math.abs(w-widths[0])<1),'复盘消息列三字等宽');
    await capture('replay-column-compact');
    const other=await page.context().newPage();
    try {
      await other.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
      await other.click('[data-width-mode="full"]');
      await page.waitForFunction(()=>document.querySelector('[data-width-mode="full"]').getAttribute('aria-pressed')==='true');
      check((await page.textContent('#step-info')).includes('当前第 2 步'),'跨标签页同步不能重置复盘步骤');
      await other.click('[data-width-mode="compact"]');
    } finally { await other.close(); }
    check(!errors.length,'排表运行错误：'+errors.join('; '));
  } finally {
    page.off('pageerror',onError);
    for (const [url,handler] of routes) await page.unroute(url,handler);
    await page.goto(`${ctx.base}/display.html?api=${encodeURIComponent(ctx.base)}`);
  }
}
