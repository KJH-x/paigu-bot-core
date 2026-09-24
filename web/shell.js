(function () {
  'use strict';

  var THEME_KEY = 'paigu.theme';
  var WORKFLOW_MS = 5000;
  var root = document.documentElement;

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

  var STEPS = [
    { n: 1, label: '开团', href: 'round.html', icon: 'flag', title: '开团 · 轮次与商品（round）' },
    { n: 2, label: '排谷', href: 'display.html', icon: 'grid', title: '排谷 · 排位/消息/名单' },
    { n: 3, label: '结算', href: 'settlement.html', icon: 'calc', title: '结算 · 试算' },
    { n: 4, label: '下单', href: 'settlement.html#order', icon: 'cart', title: '下单 / 锁定' },
    { n: 5, label: '复盘', href: 'replay.html', icon: 'replay', title: '复盘 · 重放' }
  ];

  var PAGE_DEFAULT = { index: 2, display: 2, sim: 2, admin: 1, round: 1, settings: 1, settlement: 3, replay: 5 };

  var state = { workflow: null, ok: false, step: null, timer: null };
  var listeners = [];

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

  function pageName() {
    var p = (window.location.pathname || '').split('/').pop();
    return p && p.indexOf('.') > -1 ? p : (p || 'index.html');
  }

  function pageDefault() {
    var p = pageName().replace(/\.html$/i, '');
    return PAGE_DEFAULT[p] || 2;
  }

  function apiBase() {
    if (!window.PAIGU) return '';
    var q = window.PAIGU.qs('api');
    if (q) return window.PAIGU.stripSlash(q);
    try {
      var s = window.localStorage.getItem('paigu.display.api');
      if (s) return window.PAIGU.stripSlash(s);
    } catch (e) { /* ignore */ }
    return window.PAIGU.resolveApiBase();
  }

  function badge(id, kind, text, dot) {
    var d = dot === false ? '' : '<span class="dot"></span>';
    return '<span class="status-badge ' + (kind || '') + '" id="' + id + '">' + d + esc(text) + '</span>';
  }

  function renderBadges(wf, down) {
    var host = $('wf-status');
    if (!host) return;
    if (down || !wf) {
      host.innerHTML = badge('wf-api', 'bad', '未连接', true) +
        '<span class="status-badge">工作流不可用</span>';
      return;
    }
    var gw = wf.gateway || {};
    var clients = gw.clients != null ? gw.clients : '—';
    var listenKind = gw.listening === true ? 'ok' : (gw.listening === false ? 'warn' : '');
    var version = wf.version != null ? wf.version : '—';
    var locked = !!wf.locked;
    var phase = wf.phase == null
      ? badge('wf-phase', 'warn', '阶段未配置', true)
      : badge('wf-phase', 'ok', wf.phase_label || wf.phase, true);
    host.innerHTML =
      badge('wf-api', 'ok', 'API 正常', true) +
      badge('wf-clients', listenKind, 'NapCat ' + clients, true) +
      badge('wf-version', '', '版本 v' + version, false) +
      badge('wf-locked', locked ? 'warn' : '', '锁定：' + (locked ? '是' : '否'), false) +
      phase;
  }

  /** 流程进度（弱标记）：由 /api/workflow 的 phase/locked 推导，**不**决定「当前模块」高亮。 */
  function phaseStep(wf) {
    if (!wf) return null;
    if (wf.locked === true) return 4;
    var p = String(wf.phase == null ? '' : wf.phase).toLowerCase();
    if (p === 'locked') return 4;
    if (p === 'settling') return 3;
    if (p === 'phasei' || p === 'phase1' || p === 'phaseii' || p === 'phase2' ||
        p === 'phaseiii' || p === 'phase3') return 2;
    if (p === 'phase0') return 1;
    return null;
  }

  /** 「当前模块」高亮（唯一的强样式）。 */
  function setActiveStep(n) {
    if (n == null) return;
    state.step = n;
    var i;
    var steps = document.querySelectorAll('#wf-stepper .step');
    for (i = 0; i < steps.length; i++) {
      steps[i].classList.toggle('active', +steps[i].getAttribute('data-step') === n);
    }
    var tabs = document.querySelectorAll('.mobile-tabs .mtab');
    for (i = 0; i < tabs.length; i++) {
      tabs[i].classList.toggle('active', +tabs[i].getAttribute('data-step') === n);
    }
  }

  /** 阶段进度（弱元素）：细下划线 + 小圆点，避免与 hover / active 争抢注意力。 */
  function setPhaseProgress(n) {
    var steps = document.querySelectorAll('#wf-stepper .step');
    for (var i = 0; i < steps.length; i++) {
      var sn = +steps[i].getAttribute('data-step');
      steps[i].classList.toggle('phase-now', n != null && sn === n);
      steps[i].classList.toggle('phase-done', n != null && sn < n);
    }
  }

  function emit() {
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i](state.ok ? state.workflow : null, state); } catch (e) { /* ignore */ }
    }
  }

  function fetchWorkflow() {
    if (!window.PAIGU) return;
    var base = apiBase();
    if (!base) { renderBadges(null, true); return; }
    window.PAIGU.get(base + '/api/workflow').then(function (wf) {
      state.workflow = wf;
      state.ok = true;
      renderBadges(wf, false);
      setPhaseProgress(phaseStep(wf));
      emit();
    }).catch(function () {
      state.workflow = null;
      state.ok = false;
      renderBadges(null, true);
      setPhaseProgress(null);
      emit();
    });
  }

  function scheduleFetch() {
    if (state.timer) window.clearTimeout(state.timer);
    state.timer = window.setTimeout(function () {
      if (!document.hidden) fetchWorkflow();
      scheduleFetch();
    }, WORKFLOW_MS);
  }

  function renderStepper() {
    var host = $('wf-stepper');
    if (!host) return;
    var html = '';
    for (var i = 0; i < STEPS.length; i++) {
      var s = STEPS[i];
      html += '<a class="step" data-step="' + s.n + '" href="' + s.href + '" title="' + esc(s.title || s.label) + '">' +
        '<span class="num">' + s.n + '</span>' +
        '<span class="lbl">' + esc(s.label) + '</span>' +
        '</a>';
    }
    host.innerHTML = html;
  }

  function renderMobileTabs() {
    var nav = $('mobile-tabs');
    if (!nav) {
      nav = document.createElement('nav');
      nav.id = 'mobile-tabs';
      nav.className = 'mobile-tabs';
      document.body.appendChild(nav);
    }
    var html = '';
    for (var i = 0; i < STEPS.length; i++) {
      var s = STEPS[i];
      html += '<a class="mtab" data-step="' + s.n + '" href="' + s.href + '">' +
        '<span class="ic">' + ICON[s.icon] + '</span>' +
        '<span class="mlbl">' + esc(s.label) + '</span>' +
        '</a>';
    }
    nav.innerHTML = html;
  }

  function samePage(href) {
    var h = String(href || '').split('#')[0];
    return !h || h === pageName();
  }

  function applyHash() {
    var h = (window.location.hash || '').replace(/^#/, '');
    if (!h) return;
    var el = null;
    try { el = document.getElementById(h); } catch (e) { return; }
    if (!el) return;
    if (el.scrollIntoView) el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    el.classList.add('hash-focus');
    window.setTimeout(function () { el.classList.remove('hash-focus'); }, 1500);
  }

  function bindNav() {
    var nodes = document.querySelectorAll('#wf-stepper .step, .mobile-tabs .mtab, .section-nav a');
    for (var i = 0; i < nodes.length; i++) {
      nodes[i].addEventListener('click', function (e) {
        var a = e.target && e.target.closest ? e.target.closest('a') : null;
        if (!a) return;
        var href = a.getAttribute('href') || '';
        if (!samePage(href)) return;
        e.preventDefault();
        var idx = href.indexOf('#');
        var hash = idx >= 0 ? href.slice(idx + 1) : '';
        if (hash) window.location.hash = hash;
        else if (window.scrollTo) window.scrollTo({ top: 0, behavior: 'smooth' });
        applyHash();
      });
    }
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
          var rx = n.shape === 'stadium' ? h / 2 : 8;
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

  function wireThemeToggle() {
    var btn = $('theme-toggle');
    paintThemeToggle();
    if (!btn) return;
    btn.addEventListener('click', function () {
      setTheme(currentTheme() === 'dark' ? 'light' : 'dark', true);
    });
  }

  function init() {
    installMermaidLite();
    renderStepper();
    renderMobileTabs();
    renderBadges(null, true);
    setActiveStep(pageDefault());
    wireThemeToggle();
    initCollapse();
    initColTabs();
    bindNav();
    window.addEventListener('hashchange', applyHash);
    var apiInput = $('api');
    if (apiInput) {
      if (!apiInput.value) apiInput.value = apiBase();
      apiInput.addEventListener('change', function () { fetchWorkflow(); });
    }
    document.addEventListener('visibilitychange', function () {
      if (!document.hidden) fetchWorkflow();
    });
    fetchWorkflow();
    scheduleFetch();
    if (document.readyState === 'complete') applyHash();
    else window.addEventListener('load', applyHash);
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

  window.PAIGU_SHELL = {
    refresh: fetchWorkflow,
    getState: function () {
      return { ok: state.ok, workflow: state.workflow, step: state.step };
    },
    setActiveStep: setActiveStep,
    setPhaseProgress: setPhaseProgress,
    relDay: relDay,
    onWorkflow: function (fn) {
      if (typeof fn !== 'function') return function () {};
      listeners.push(fn);
      if (state.ok && state.workflow) {
        try { fn(state.workflow, state); } catch (e) { /* ignore */ }
      }
      return function () {
        var idx = listeners.indexOf(fn);
        if (idx >= 0) listeners.splice(idx, 1);
      };
    },
    theme: {
      get: currentTheme,
      set: function (t) { setTheme(t, true); }
    }
  };

  function onReady(fn) {
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', fn);
    else fn();
  }

  onReady(init);
})();
