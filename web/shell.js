(function () {
  'use strict';
  var THEME_KEY='paigu.theme', WORKFLOW_MS=5000, root=document.documentElement;
  var ICON = {
    logo: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7.5l9-4 9 4-9 4-9-4z"/><path d="M3 7.5v9l9 4 9-4v-9"/><path d="M12 11.5v9"/></svg>',
    flag: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M5 21V4"/><path d="M5 4h11l-1.6 4L16 12H5"/></svg>',
    grid: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.6"/><rect x="14" y="3" width="7" height="7" rx="1.6"/><rect x="3" y="14" width="7" height="7" rx="1.6"/><rect x="14" y="14" width="7" height="7" rx="1.6"/></svg>',
    calc: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="3" width="14" height="18" rx="2"/><path d="M8.5 7.5h7"/><path d="M8.5 12h.01M12 12h.01M15.5 12h.01M8.5 15.5h.01M12 15.5h.01M15.5 15.5h.01"/><path d="M8.5 18.5h7"/></svg>',
    cart: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="20" r="1.4"/><circle cx="18" cy="20" r="1.4"/><path d="M2 3h3l2.4 12.4a2 2 0 0 0 2 1.6h8.3a2 2 0 0 0 2-1.6L21 7H6"/></svg>',
    replay: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 12a8.5 8.5 0 1 0 2.8-6.3"/><path d="M3 4v5h5"/><path d="M12 8.5V12l2.8 1.8"/></svg>',
    sun: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4l1.4-1.4M18 6l1.4-1.4"/></svg>',
    moon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>'
  };


  var STEPS=[
    {n:0,key:'index',label:'总览',icon:'logo'},
    {n:1,key:'round',label:'轮次与商品',icon:'flag'},
    {n:2,key:'display',label:'排谷',icon:'grid'},
    {n:3,key:'settlement',label:'结算与下单',icon:'calc'},
    {n:5,key:'replay',label:'复盘',icon:'replay'},
    {n:6,key:'members',label:'成员',icon:'grid'},
    {n:7,key:'settings',label:'系统设置',icon:'calc'}
  ];
  var state={workflow:null,ok:false,step:null,timer:null,generation:0}, listeners=[];
  function $(id){return document.getElementById(id);}
  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  function $(id) { return document.getElementById(id); }

  function readTheme() {
    try {
      var v = window.localStorage.getItem(THEME_KEY);
      if (v === 'dark' || v === 'light') return v;
    } catch (e) { /* ignore */ }
    try {
      if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) return 'dark';
    } catch (e2) { /* ignore */ }
    return 'light';
  }

  function currentTheme() {
    return root.getAttribute('data-theme') === 'dark' ? 'dark' : 'light';
  }

  function paintThemeToggle() {
    var btn = $('theme-toggle');
    if (!btn) return;
    var dark = currentTheme() === 'dark';
    btn.innerHTML = dark ? ICON.sun : ICON.moon;
    btn.setAttribute('title', dark ? '切换到浅色主题' : '切换到深色主题');
    btn.setAttribute('aria-label', dark ? '切换到浅色主题' : '切换到深色主题');
  }

  function setTheme(t, persist) {
    t = t === 'dark' ? 'dark' : 'light';
    root.setAttribute('data-theme', t);
    if (persist) {
      try { window.localStorage.setItem(THEME_KEY, t); } catch (e) { /* ignore */ }
    }
    paintThemeToggle();
  }

  setTheme(readTheme(), false);


  function pageKey(){return document.body.dataset.page || 'display';}
  function apiBase(){return window.PAIGU.resolveApiBase();}
  function badge(id,kind,text){
    return '<span class="status-badge '+(kind || '')+'" id="'+id+'">'+esc(text)+'</span>';
  }
  function renderBadges(wf,down){
    var host=$('wf-status');if(!host)return;
    if(down || !wf){host.innerHTML=badge('wf-api','bad','API 未连接')+badge('wf-clients','warn','接收器状态未知');return;}
    var gw=wf.gateway || {}, clients=gw.clients, locked=!!wf.locked;
    host.innerHTML=
      badge('wf-round','','激活轮：'+(wf.title || wf.round_id || '—')+(wf.category==='test'?' · 测试':' · 正常'))+
      badge('wf-api','ok','API 正常')+
      badge('wf-listening',gw.listening?'ok':'warn',gw.listening?'接入监听中':'接入未监听')+
      badge('wf-clients',Number(clients)>0?'ok':'warn',Number(clients)>0?'接收器 '+clients:'未连接接收器')+
      badge('wf-version','','版本 v'+(wf.version == null?'—':wf.version))+
      badge('wf-locked',locked?'warn':'',wf.lock_error?'锁状态不可读 · 暂停写入':'锁定：'+(locked?'是':'否'))+
      badge('wf-phase',wf.phase==null?'warn':'',wf.phase_label || wf.phase || '阶段未配置');
    $('wf-round').title='稳定标识：'+wf.round_id+'；这里只显示激活轮，不会切换轮次';
    host.title='状态读取：'+(wf.updated_at || '未知')+'（不是最后收到消息的时间）';
  }
  function phaseStep(wf){
    if(!wf)return null;
    if(wf.locked || /^(locked|settling)$/i.test(wf.phase || ''))return 3;
    if(/^phase(iii|ii|i|3|2|1)$/i.test(wf.phase || ''))return 2;
    return wf.phase==='Phase0'?1:null;
  }
  function setActiveStep(n){
    state.step=n;
    document.querySelectorAll('#wf-stepper .step,.mobile-tabs .mtab').forEach(function(a){
      var active=Number(a.dataset.step)===n;a.classList.toggle('active',active);
      if(active)a.setAttribute('aria-current','page');else a.removeAttribute('aria-current');
    });
  }
  function setPhaseProgress(n){
    document.querySelectorAll('#wf-stepper .step').forEach(function(a){
      var i=Number(a.dataset.step), business=i>0 && i<6;
      a.classList.toggle('phase-now',business && n!=null && i===n);
      a.classList.toggle('phase-done',business && n!=null && i<n);
    });
  }
  function emit(){listeners.forEach(function(fn){try{fn(state.ok?state.workflow:null,state);}catch(e){}});}
  function fetchWorkflow(){
    var base=apiBase(), generation=++state.generation;
    return window.PAIGU.get(base+'/api/workflow').then(function(wf){
      if(base!==apiBase() || generation!==state.generation)return;
      state.workflow=wf;state.ok=true;renderBadges(wf,false);setPhaseProgress(phaseStep(wf));emit();
    }).catch(function(){
      if(base!==apiBase() || generation!==state.generation)return;
      state.workflow=null;state.ok=false;renderBadges(null,true);setPhaseProgress(null);emit();
    });
  }
  function scheduleFetch(){
    if(state.timer)window.clearTimeout(state.timer);
    state.timer=window.setTimeout(function(){if(!document.hidden)fetchWorkflow();scheduleFetch();},WORKFLOW_MS);
  }
  function renderNavigation(){
    var host=$('wf-stepper');
    if(host){
      host.setAttribute('aria-label','全站工作区');
      host.innerHTML=STEPS.map(function(s){
        return '<a class="step'+(s.n===6?' auxiliary':'')+'" data-step="'+s.n+'" href="'+esc(window.PAIGU.pageHref(s.key+'.html'))+'">'+
          '<span class="workspace-icon" aria-hidden="true">'+ICON[s.icon]+'</span><span class="lbl">'+s.label+'</span></a>';
      }).join('');
    }
    var mobile=$('mobile-tabs');
    if(!mobile){mobile=document.createElement('nav');mobile.id='mobile-tabs';mobile.className='mobile-tabs';document.body.appendChild(mobile);}
    mobile.setAttribute('aria-label','全站工作区（窄屏）');
    mobile.innerHTML=STEPS.map(function(s){return '<a class="mtab" data-step="'+s.n+'" href="'+esc(window.PAIGU.pageHref(s.key+'.html'))+'"><span class="ic">'+ICON[s.icon]+'</span><span class="mlbl">'+s.label+'</span></a>';}).join('');
    var active=STEPS.find(function(s){return s.key===pageKey();});setActiveStep(active?active.n:2);
  }
  function applyHash(){
    var hash=window.location.hash;
    var tabs=document.querySelector('.section-nav[data-panels]');
    if(tabs){
      var links=Array.from(tabs.querySelectorAll('a')), selected=links.find(function(a){return new URL(a.href).hash===hash;}) || links[0];
      links.forEach(function(a){var panel=document.getElementById(new URL(a.href).hash.slice(1));if(panel)panel.hidden=a!==selected;});
      if(selected && !hash)hash=new URL(selected.href).hash;
    }
    document.querySelectorAll('.section-nav a').forEach(function(a){
      var active=new URL(a.href).hash===hash || (a.dataset.alias || '').split(',').indexOf(hash)>=0;a.classList.toggle('active',active);
      if(active)a.setAttribute('aria-current','location');else a.removeAttribute('aria-current');
    });
    if(!hash)return;
    var el=document.getElementById(hash.slice(1));
    var notice=$('unknown-anchor');if(notice)notice.remove();
    if(!el){
      var main=document.querySelector('main');if(main){notice=document.createElement('p');notice.id='unknown-anchor';notice.className='workspace-notice warning';notice.setAttribute('role','status');notice.textContent='原位置已调整。请使用上方工作区或页内导航继续；当前页面与数据未改变。';main.prepend(notice);}return;
    }
    var fold=el.closest('.collapsed');
    if(fold){fold.classList.remove('collapsed');var toggle=fold.querySelector('[data-collapse]');if(toggle)toggle.setAttribute('aria-expanded','true');}
    el.scrollIntoView({block:'start'});el.classList.add('hash-focus');
    window.setTimeout(function(){el.classList.remove('hash-focus');},1500);
  }
  function normalizeLinks(){
    document.querySelectorAll('a[href]').forEach(function(a){
      var raw=a.getAttribute('href');if(!raw || raw[0]==='#')return;
      var url=new URL(raw,window.location.href), key=url.pathname.split('/').pop().replace(/\.html$/i,'');
      if(url.origin!==window.location.origin || !STEPS.some(function(s){return s.key===key;}) && key!=='admin')return;
      var next=window.PAIGU.pageHref(url.href);if(a.href!==next)a.href=next;
    });
  }
  function bindNav(){
    normalizeLinks();
    document.addEventListener('click',function(e){
      var a=e.target.closest && e.target.closest('a[href]');
      if(!a || e.defaultPrevented || e.button!==0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || a.target==='_blank')return;
      var url=new URL(a.href), current=new URL(window.location.href), key=url.pathname.split('/').pop().replace(/\.html$/i,'');
      if(url.origin!==current.origin || url.searchParams.get('api') && url.searchParams.get('api')!==apiBase())return;
      if(key!==pageKey() && url.pathname!==current.pathname)return;
      if(!url.hash && a.closest('#wf-stepper,.mobile-tabs,.brand')){e.preventDefault();return;}
      if(url.hash){e.preventDefault();window.location.hash=url.hash;applyHash();}
    });
    if(window.MutationObserver)new MutationObserver(normalizeLinks).observe(document.body,{childList:true,subtree:true});
  }
  function legacyEntry(){
    var hash=window.location.hash, target=null;
    if(pageKey()==='settings' && hash==='#sec-members')target='members.html#sec-members';
    if(pageKey()==='settings' && hash==='#sec-messages' || pageKey()==='index' && hash==='#sec-msglog')target='display.html#sec-msglog';
    if(target){window.location.replace(window.PAIGU.pageHref(target));return true;}return false;
  }
  function organizeChrome(){
    var header=document.querySelector('.app-topbar');if(!header)return;
    var row=header.querySelector('.shell-row'), tools=header.querySelector('.tools-row');if(!row || !tools)return;
    document.querySelectorAll('.nav-links').forEach(function(n){n.remove();});
    var brand=row.querySelector('.brand');
    if(brand){brand.innerHTML='<span class="brand-logo">'+ICON.logo+'</span>';brand.setAttribute('aria-label','总览');brand.title='排谷 · 总览';brand.href=window.PAIGU.pageHref('index.html');}
    var current=STEPS.find(function(s){return s.key===pageKey();});
    var title=document.createElement('h1');title.id='workspace-title';title.textContent=current?current.label:'排谷';tools.insertBefore(title,tools.firstChild);
    var status=$('wf-status');if(status)tools.insertBefore(status,title.nextSibling);
    var input=$('api'), connection=input && input.closest('label'), theme=$('theme-toggle');
    if(connection){connection.classList.add('connection-field');row.insertBefore(connection,theme);}
    var main=document.querySelector('main');
    if(!main){main=document.createElement('main');main.id='main-content';while(header.nextSibling)main.appendChild(header.nextSibling);header.parentNode.appendChild(main);}
    main.setAttribute('tabindex','-1');
    var skip=document.createElement('a');skip.className='skip-link';skip.href='#main-content';skip.textContent='跳到工作区';document.body.insertBefore(skip,document.body.firstChild);
    function measure(){root.style.setProperty('--shell-height',Math.ceil(header.getBoundingClientRect().height)+'px');}
    measure();if(window.ResizeObserver)new ResizeObserver(measure).observe(header);else window.addEventListener('resize',measure);
  }
  function initCollapse() {
    var nodes = document.querySelectorAll('button[data-collapse]');
    for (var i = 0; i < nodes.length; i++) {
      (function (btn) {
        btn.addEventListener('click', function () {
          var panel = btn.closest ? btn.closest('.panel') : null;
          if (!panel) return;
          panel.classList.toggle('collapsed');
          btn.setAttribute('aria-expanded', panel.classList.contains('collapsed') ? 'false' : 'true');
        });
      })(nodes[i]);
    }
  }

  function initColTabs() {
    var bar = document.querySelector('.col-tabs');
    if (!bar) return;
    var btns = bar.querySelectorAll('button[data-col-target]');
    if (!btns.length) return;
    function activate(name) {
      for (var i = 0; i < btns.length; i++) {
        btns[i].classList.toggle('active', btns[i].getAttribute('data-col-target') === name);
      }
      var cols = document.querySelectorAll('[data-col]');
      for (var j = 0; j < cols.length; j++) {
        cols[j].classList.toggle('is-active', cols[j].getAttribute('data-col') === name);
      }
    }
    bar.addEventListener('click', function (e) {
      var b = e.target && e.target.closest ? e.target.closest('button[data-col-target]') : null;
      if (b) activate(b.getAttribute('data-col-target'));
    });
    activate(btns[0].getAttribute('data-col-target'));
  }

  function installMermaidLite() {
    if (window.mermaid) return;

    function parse(def) {
      var nodes = {}, order = [], edges = [], hlNodes = {}, hlEdges = {};
      function addNode(id, label, shape) {
        if (nodes[id]) return;
        nodes[id] = { id: id, label: label, shape: shape };
        order.push(id);
      }
      var lines = String(def == null ? '' : def).split('\n');
      var rectRe = /^([A-Za-z0-9_]+)\s*\["(.*)"\]$/;
      var diamondRe = /^([A-Za-z0-9_]+)\s*\{\{"(.*)"\}\}$/;
      var stadiumRe = /^([A-Za-z0-9_]+)\s*\(\["(.*)"\]\)$/;
      var edgeRe = /^([A-Za-z0-9_]+)\s*-->\s*([A-Za-z0-9_]+)$/;
      for (var i = 0; i < lines.length; i++) {
        var ln = lines[i].trim();
        if (!ln) continue;
        var m;
        if (/^graph\b/i.test(ln)) continue;
        if ((m = rectRe.exec(ln))) { addNode(m[1], m[2], 'rect'); continue; }
        if ((m = diamondRe.exec(ln))) { addNode(m[1], m[2], 'diamond'); continue; }
        if ((m = stadiumRe.exec(ln))) { addNode(m[1], m[2], 'stadium'); continue; }
        if ((m = edgeRe.exec(ln))) { edges.push({ from: m[1], to: m[2] }); continue; }
        if (/^classDef\b/.test(ln)) continue;
        if ((m = /^class\s+([A-Za-z0-9_,]+)\s+\w+\s*;?$/.exec(ln))) {
          var ids = m[1].split(',');
          for (var a = 0; a < ids.length; a++) { if (ids[a]) hlNodes[ids[a]] = true; }
          continue;
        }
        if ((m = /^linkStyle\s+([\d,\s]+)/.exec(ln))) {
          var nums = m[1].split(',');
          for (var b = 0; b < nums.length; b++) {
            var x = nums[b].trim();
            if (x !== '') hlEdges[+x] = true;
          }
          continue;
        }
      }
      return { nodes: nodes, order: order, edges: edges, hlNodes: hlNodes, hlEdges: hlEdges };
    }

    function layout(g) {
      var indeg = {}, adj = {};
      var i;
      for (i = 0; i < g.order.length; i++) { indeg[g.order[i]] = 0; adj[g.order[i]] = []; }
      for (i = 0; i < g.edges.length; i++) {
        var e = g.edges[i];
        if (adj[e.from] && indeg[e.to] != null) { adj[e.from].push(e.to); indeg[e.to]++; }
      }
      var depth = {}, queue = [];
      for (i = 0; i < g.order.length; i++) {
        if (!indeg[g.order[i]]) { depth[g.order[i]] = 0; queue.push(g.order[i]); }
      }
      if (!queue.length && g.order.length) { depth[g.order[0]] = 0; queue.push(g.order[0]); }
      var qi = 0;
      while (qi < queue.length) {
        var id = queue[qi++];
        var out = adj[id] || [];
        for (var j = 0; j < out.length; j++) {
          var nx = out[j];
          var d = (depth[id] || 0) + 1;
          if (depth[nx] == null || d > depth[nx]) depth[nx] = d;
          indeg[nx]--;
          if (indeg[nx] <= 0) queue.push(nx);
        }
      }
      for (i = 0; i < g.order.length; i++) { if (depth[g.order[i]] == null) depth[g.order[i]] = 0; }
      var byDepth = {}, maxD = 0, maxCount = 1;
      for (i = 0; i < g.order.length; i++) {
        var dd = depth[g.order[i]];
        (byDepth[dd] = byDepth[dd] || []).push(g.order[i]);
        if (dd > maxD) maxD = dd;
      }
      for (var k in byDepth) { if (byDepth[k].length > maxCount) maxCount = byDepth[k].length; }
      var colW = 172, rowH = 54, padX = 26, padY = 22;
      var W = padX * 2 + (maxD + 1) * colW;
      var H = padY * 2 + maxCount * rowH;
      var pos = {};
      for (var kk in byDepth) {
        var d2 = +kk, arr = byDepth[kk], total = arr.length;
        for (var t = 0; t < total; t++) {
          pos[arr[t]] = {
            x: padX + d2 * colW + colW / 2,
            y: padY + (t + (maxCount - total) / 2) * rowH + rowH / 2
          };
        }
      }
      return { pos: pos, W: W, H: H };
    }

    function render(def) {
      var g = parse(def);
      if (!g.order.length) {
        return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 220 60" width="220" height="60">' +
          '<text x="10" y="34" font-size="12" fill="currentColor">无节点</text></svg>';
      }
      var L = layout(g);
      var p = [];
      p.push('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ' + L.W + ' ' + L.H + '" ' +
        'width="' + L.W + '" height="' + L.H + '" style="max-width:100%;height:auto" role="img">');
      p.push('<defs><marker id="wf-arw" markerWidth="10" markerHeight="10" refX="9" refY="3" ' +
        'orient="auto" markerUnits="strokeWidth"><path d="M0,0 L9,3 L0,6 z" fill="currentColor"/></marker></defs>');
      var i;
      for (i = 0; i < g.edges.length; i++) {
        var e = g.edges[i];
        var a = L.pos[e.from], b = L.pos[e.to];
        if (!a || !b) continue;
        var hl = !!g.hlEdges[i];
        p.push('<line x1="' + a.x + '" y1="' + a.y + '" x2="' + b.x + '" y2="' + b.y + '" ' +
          'stroke="' + (hl ? '#dc2626' : 'currentColor') + '" stroke-width="' + (hl ? 3 : 1.2) + '" ' +
          'opacity="' + (hl ? 1 : .45) + '" marker-end="url(#wf-arw)"/>');
      }
      for (i = 0; i < g.order.length; i++) {
        var id = g.order[i], n = g.nodes[id], pos = L.pos[id];
        if (!pos) continue;
        var w = Math.max(72, Math.min(152, 18 + String(n.label).length * 8));
        var h = 30;
        var hl2 = !!g.hlNodes[id];
        var fill = hl2 ? '#ffe08a' : 'var(--panel-2,#f7f9fc)';
        var stroke = hl2 ? '#d97706' : 'var(--border-strong,#c4cfdd)';
        var sw = hl2 ? 3 : 1.2;
        if (n.shape === 'diamond') {
          p.push('<polygon points="' + pos.x + ',' + (pos.y - h / 2 - 5) + ' ' + (pos.x + w / 2) + ',' + pos.y +
            ' ' + pos.x + ',' + (pos.y + h / 2 + 5) + ' ' + (pos.x - w / 2) + ',' + pos.y +
            '" style="fill:' + fill + ';stroke:' + stroke + ';stroke-width:' + sw + '"/>');
        } else {
          var rx = 0;
          p.push('<rect x="' + (pos.x - w / 2) + '" y="' + (pos.y - h / 2) + '" width="' + w + '" height="' + h +
            '" rx="' + rx + '" style="fill:' + fill + ';stroke:' + stroke + ';stroke-width:' + sw + '"/>');
        }
        var label = String(n.label);
        if (label.length > 16) label = label.slice(0, 15) + '…';
        p.push('<text x="' + pos.x + '" y="' + (pos.y + 4) + '" text-anchor="middle" font-size="11" ' +
          'fill="' + (hl2 ? '#7c2d12' : 'currentColor') + '">' + esc(label) + '</text>');
      }
      p.push('</svg>');
      return p.join('');
    }

    window.mermaid = {
      initialize: function () { return undefined; },
      render: function (id, def) {
        return new Promise(function (resolve, reject) {
          try { resolve({ svg: render(def) }); }
          catch (e) { reject(e); }
        });
      }
    };
  }


  function pad2(n) { return (n < 10 ? '0' : '') + n; }

  /**
   * 相对日提示：`周几 + 前天/昨天/今天/明天/后天`（仅这几种），
   * 其他情况显示 `X 天前 / X 天后`；附带 HH:MM。
   */
  function relDay(ms) {
    var t = Number(ms);
    if (!isFinite(t) || t <= 0) return '';
    var d = new Date(t);
    var now = new Date();
    var a = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    var b = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    var diff = Math.round((a - b) / 86400000);
    var rel;
    if (diff === -2) rel = '前天';
    else if (diff === -1) rel = '昨天';
    else if (diff === 0) rel = '今天';
    else if (diff === 1) rel = '明天';
    else if (diff === 2) rel = '后天';
    else rel = Math.abs(diff) + ' 天' + (diff < 0 ? '前' : '后');
    var wd = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][d.getDay()];
    return wd + ' ' + rel + ' ' + pad2(d.getHours()) + ':' + pad2(d.getMinutes());
  }


  function init(){
    if(legacyEntry())return;
    organizeChrome();installMermaidLite();renderNavigation();renderBadges(null,true);
    paintThemeToggle();var theme=$('theme-toggle');if(theme)theme.addEventListener('click',function(){setTheme(currentTheme()==='dark'?'light':'dark',true);});
    initCollapse();initColTabs();bindNav();
    window.addEventListener('hashchange',applyHash);
    var api=$('api');if(api){
      api.value=apiBase();
      api.addEventListener('change',function(e){
        e.stopImmediatePropagation();
        var value=api.value.trim(), parsed;
        try{parsed=new URL(value);if(!/^https?:$/.test(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash)throw new Error('invalid');}
        catch(err){api.value=apiBase();window.PAIGU.toast('请输入不含密钥或查询参数的 HTTP(S) API 地址','bad');return;}
        value=window.PAIGU.stripSlash(value);if(value===apiBase()){api.value=value;return;}
        var next=new URL(window.location.href);next.searchParams.set('api',value);
        window.PAIGU.protectAction(function(){
          try{window.localStorage.setItem('paigu.display.api',value);}catch(err){}
          window.PAIGU.navigate(next.href);
        }).then(function(proceeded){if(!proceeded)api.value=apiBase();}).catch(function(err){api.value=apiBase();window.PAIGU.toast(window.PAIGU.errorText(err),'bad');});
      },true);
    }
    document.addEventListener('visibilitychange',function(){if(!document.hidden)fetchWorkflow();});
    fetchWorkflow();scheduleFetch();
    if(document.readyState==='complete')applyHash();else window.addEventListener('load',applyHash);
  }
  window.PAIGU_SHELL={
    refresh:fetchWorkflow,getState:function(){return {ok:state.ok,workflow:state.workflow,step:state.step};},
    setActiveStep:setActiveStep,setPhaseProgress:setPhaseProgress,relDay:relDay,
    onWorkflow:function(fn){if(typeof fn!=='function')return function(){};listeners.push(fn);if(state.ok && state.workflow)fn(state.workflow,state);return function(){var i=listeners.indexOf(fn);if(i>=0)listeners.splice(i,1);};},
    theme:{get:currentTheme,set:function(t){setTheme(t,true);}}
  };
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init);else init();
})();
