(function () {
  'use strict';

  var P = window.PAIGU;
  var esc = P.esc;

  function $(id) { return document.getElementById(id); }

  /* 内置占位示例（脱敏：甲/乙/丙/丁/戊）。默认数据源为同目录 replay_view.json（可选），
     读取失败时回退到本示例。示例与 sample_replay.json 结构等价。 */
  var EMBEDDED_SAMPLE = {
    sample_id: '占位示例',
    title: '占位示例',
    items: [
      {
        item_id: '月行水上/特典卡组-校园凭证', name: '特典卡组-校园凭证', kind: 'split',
        section: '特典卡组-校园凭证',
        variants: [{ variant_id: '整套', name: '整套', capacity: 5 }]
      },
      {
        item_id: '月行水上/通行证SP-月行水上', name: '通行证SP-月行水上', kind: 'single',
        section: '单领', variants: []
      },
      {
        item_id: '月行水上/徽章-月行水上', name: '徽章-月行水上', kind: 'split',
        section: '徽章-月行水上',
        variants: [
          { variant_id: '亚克力', name: '亚克力', capacity: 3 },
          { variant_id: '吧唧', name: '吧唧', capacity: 4 }
        ]
      }
    ],
    messages: [
      { seq: 1, display: '甲', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1' },
      { seq: 2, display: '乙', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1' },
      { seq: 3, display: '丙', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1' },
      { seq: 4, display: '丁', text: '排亚克力1', status: 'Applied', detail: 'claim: 徽章-月行水上 亚克力 x1' },
      { seq: 5, display: '戊', text: '排亚克力1', status: 'Applied', detail: 'claim: 徽章-月行水上 亚克力 x1' },
      { seq: 6, display: '乙', text: '撤整套1', status: 'Applied', detail: 'cancel: 特典卡组-校园凭证 整套 x1' },
      { seq: 7, display: '丁', text: '排吧唧1', status: 'Applied', detail: 'claim: 徽章-月行水上 吧唧 x1' },
      { seq: 8, display: '管理员', text: '/锁位 吧唧 3号位给甲', status: 'Applied', detail: 'admin: 包尾锁位 吧唧 slot3 -> 甲' },
      { seq: 9, display: '管理员', text: '/固定 吧唧 2号位给戊', status: 'Applied', detail: 'admin: 固定 吧唧 slot2 -> 戊' }
    ],
    steps: [
      { index: 0, message_seq: 1, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: '甲', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 1, before: null, after: '甲', reason: 'NewClaimFilled' }] },
      { index: 1, message_seq: 2, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: '甲', status: 'filled' }, { slot: 2, user: '乙', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 2, before: null, after: '乙', reason: 'NewClaimFilled' }] },
      { index: 2, message_seq: 3, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: '甲', status: 'filled' }, { slot: 2, user: '乙', status: 'filled' }, { slot: 3, user: '丙', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 3, before: null, after: '丙', reason: 'NewClaimFilled' }] },
      { index: 3, message_seq: 4, board: { '月行水上/徽章-月行水上|亚克力': [{ slot: 1, user: '丁', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '亚克力', slot: 1, before: null, after: '丁', reason: 'NewClaimFilled' }] },
      { index: 4, message_seq: 5, board: { '月行水上/徽章-月行水上|亚克力': [{ slot: 1, user: '丁', status: 'filled' }, { slot: 2, user: '戊', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '亚克力', slot: 2, before: null, after: '戊', reason: 'NewClaimFilled' }] },
      { index: 5, message_seq: 6, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: '甲', status: 'filled' }, { slot: 2, user: '丙', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 2, before: '乙', after: '丙', reason: 'AutoMovedForward' }, { item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 3, before: '丙', after: null, reason: 'CancelReleased' }] },
      { index: 6, message_seq: 7, board: { '月行水上/徽章-月行水上|吧唧': [{ slot: 1, user: '丁', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '吧唧', slot: 1, before: null, after: '丁', reason: 'NewClaimFilled' }] },
      { index: 7, message_seq: 8, board: { '月行水上/徽章-月行水上|吧唧': [{ slot: 1, user: '丁', status: 'filled' }, { slot: 3, user: '甲', status: 'locked' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '吧唧', slot: 3, before: null, after: '甲', reason: 'TailSegmentCreated' }] },
      { index: 8, message_seq: 9, board: { '月行水上/徽章-月行水上|吧唧': [{ slot: 1, user: '丁', status: 'filled' }, { slot: 2, user: '戊', status: 'filled' }, { slot: 3, user: '甲', status: 'locked' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '吧唧', slot: 2, before: null, after: '戊', reason: 'AdminFixed' }] }
    ],
    who_whats: [
      { display: '甲', identity: '甲', items: [{ name: '整套', qty: 1 }, { name: '吧唧', qty: 1 }] },
      { display: '乙', identity: '乙', items: [{ name: '整套', qty: 1 }] },
      { display: '丙', identity: '丙', items: [{ name: '整套', qty: 1 }] },
      { display: '丁', identity: '丁', items: [{ name: '亚克力', qty: 1 }, { name: '吧唧', qty: 1 }] },
      { display: '戊', identity: '戊', items: [{ name: '亚克力', qty: 1 }, { name: '吧唧', qty: 1 }] }
    ],
    expected: {
      '月行水上/特典卡组-校园凭证|整套': ['甲', '丙'],
      '月行水上/徽章-月行水上|亚克力': ['丁', '戊'],
      '月行水上/徽章-月行水上|吧唧': ['丁', '戊', '甲']
    }
  };

  var KIND_LABEL = { split: '拆分', single: '单领', gift: '赠品', shipping: '运费', adjustment: '调整' };

  var state = {
    data: null,
    source: '',
    current: 0,
    playing: false,
    speed: 1,
    timer: null,
    view: 'table',
    cumulative: [],
    lastChange: {},
    focusedKey: null,
    meta: null,
    mermaidReady: false,
    wwOpen: true
  };

  var tableEl, boardRenderer;

  function boardKey(itemId, variantId) {
    return itemId + '|' + (variantId == null ? '' : variantId);
  }

  function cellKey(key, slot) {
    return key + '#1:' + slot;
  }

  function kindLabel(k) { return KIND_LABEL[k] || k || ''; }

  function msgBySeq(seq) {
    var msgs = (state.data && state.data.messages) || [];
    for (var i = 0; i < msgs.length; i++) {
      if (msgs[i].seq === seq) return msgs[i];
    }
    return null;
  }

  function buildDerived() {
    var steps = (state.data && state.data.steps) || [];
    var boards = [];
    var cur = {};
    var lastChange = {};
    for (var i = 0; i < steps.length; i++) {
      var next = {};
      for (var k in cur) { if (Object.prototype.hasOwnProperty.call(cur, k)) next[k] = cur[k].slice(); }
      var b = steps[i].board || {};
      for (var k2 in b) {
        if (!Object.prototype.hasOwnProperty.call(b, k2)) continue;
        next[k2] = b[k2].slice();
      }
      var changed = steps[i].changed || [];
      for (var c = 0; c < changed.length; c++) {
        lastChange[cellKey(boardKey(changed[c].item_id, changed[c].variant_id), changed[c].slot)] = i;
      }
      boards.push(next);
      cur = next;
    }
    state.cumulative = boards;
    state.lastChange = lastChange;
  }

  function renderHeader() {
    if (!state.data) return;
    var steps = state.data.steps || [];
    $('replay-title').textContent = state.data.title || state.data.sample_id || '排谷重放';
    $('step-info').textContent = '共 ' + steps.length + ' 步 · 当前第 ' +
      (steps.length ? state.current + 1 : 0) + ' 步';
    $('step-count').textContent = steps.length ? (state.current + 1) + '/' + steps.length : '0/0';
    $('source-info').textContent = state.source ? '数据源：' + state.source : '';
  }

  function renderList() {
    var ul = $('step-list');
    var steps = state.data.steps || [];
    P.syncKeyedChildren(
      ul,
      steps,
      function (s, i) { return 'step' + i; },
      function () {
        var li = document.createElement('li');
        li.className = 'step-item';
        li.innerHTML = '<span class="seq"></span><span class="who"></span>' +
          '<span class="txt"></span><span class="status"></span>';
        li.__seq = li.querySelector('.seq');
        li.__who = li.querySelector('.who');
        li.__txt = li.querySelector('.txt');
        li.__st = li.querySelector('.status');
        return li;
      },
      function (li, step, key) {
        var i = parseInt(key.slice(4), 10);
        var msg = msgBySeq(step.message_seq);
        var seq = msg ? msg.seq : step.message_seq;
        var display = msg ? (msg.display || msg.user) : '?';
        var text = msg ? msg.text : '';
        var status = msg ? (msg.status || '') : '';
        li.className = 'step-item' + (i === state.current ? ' current' : '');
        li.__seq.textContent = '#' + seq;
        li.__who.textContent = display;
        li.__txt.textContent = text;
        li.__txt.title = text;
        li.__st.textContent = status;
        li.__st.className = 'status' + (status ? ' st-' + String(status).toLowerCase() : '');
        li.onclick = function () { setPlaying(false); goToStep(i); };
      }
    );
    var curEl = ul.querySelector('.current');
    if (curEl && curEl.scrollIntoView) curEl.scrollIntoView({ block: 'nearest' });
  }

  function applyFocus() {
    var cells = tableEl.querySelectorAll('.cell');
    for (var i = 0; i < cells.length; i++) {
      if (cells[i].__k) cells[i].classList.toggle('focused', cells[i].__k === state.focusedKey);
    }
  }

  function renderBoard() {
    var step = (state.data.steps || [])[state.current];
    var board = state.cumulative[state.current] || {};
    var rows = P.normalizeBoard({ board: board }, state.meta);
    var changeMap = P.normalizeChanged({ changed: (step && step.changed) || [] });
    boardRenderer.render(rows, changeMap);
    applyFocus();
  }

  function mermaidLabel(s) {
    return String(s == null ? '' : s)
      .replace(/"/g, '#quot;')
      .replace(/[\[\]{}()<>|]/g, ' ')
      .trim();
  }

  function buildMermaid() {
    var data = state.data;
    var step = (data.steps || [])[state.current];
    var changes = (step && step.changed) || [];
    var board = state.cumulative[state.current] || {};

    var changedItemIds = {};
    var changedVariantKeys = {};
    var changedUserKeys = {};
    for (var i = 0; i < changes.length; i++) {
      var c = changes[i];
      var ck = boardKey(c.item_id, c.variant_id);
      changedItemIds[c.item_id] = true;
      changedVariantKeys[ck] = true;
      if (c.before) changedUserKeys[ck + '||' + c.before] = true;
      if (c.after) changedUserKeys[ck + '||' + c.after] = true;
    }

    var nodeLines = [];
    var edges = [];
    var hlNodes = {};
    var seq = 0;
    function nextId() { return 'n' + (seq++); }
    var itemIds = {};
    var variantIds = {};
    var userIds = {};
    var items = data.items || [];

    for (var a = 0; a < items.length; a++) {
      var item = items[a];
      var iid = itemIds[item.item_id];
      if (!iid) { iid = nextId(); itemIds[item.item_id] = iid; }
      nodeLines.push(iid + '["' + mermaidLabel(item.name) + '"]');
      if (changedItemIds[item.item_id]) hlNodes[iid] = true;

      var hasVariants = !!(item.variants && item.variants.length);
      var variants = hasVariants
        ? item.variants
        : [{ variant_id: '', name: (item.kind === 'single' ? '单领' : '默认'), capacity: 0 }];

      for (var v = 0; v < variants.length; v++) {
        var variant = variants[v];
        var key = boardKey(item.item_id, variant.variant_id);
        var slots = board[key] || [];
        var users = [];
        var seen = {};
        for (var s = 0; s < slots.length; s++) {
          var u = slots[s] && slots[s].user;
          if (u && slots[s].status !== 'empty' && !seen[u]) { seen[u] = true; users.push(u); }
        }

        if (hasVariants) {
          var vid = variantIds[key];
          if (!vid) { vid = nextId(); variantIds[key] = vid; }
          nodeLines.push(vid + '{{"' + mermaidLabel(variant.name) + '"}}');
          if (changedVariantKeys[key]) hlNodes[vid] = true;
          edges.push({ from: iid, to: vid, changed: !!changedVariantKeys[key] });
          for (var uu = 0; uu < users.length; uu++) {
            var uk = key + '||' + users[uu];
            var uid = userIds[uk];
            if (!uid) { uid = nextId(); userIds[uk] = uid; }
            nodeLines.push(uid + '(["' + mermaidLabel(users[uu]) + '"])');
            var chU = !!changedUserKeys[uk];
            if (chU) hlNodes[uid] = true;
            edges.push({ from: vid, to: uid, changed: chU });
          }
        } else {
          for (var u2 = 0; u2 < users.length; u2++) {
            var uk2 = key + '||' + users[u2];
            var uid2 = userIds[uk2];
            if (!uid2) { uid2 = nextId(); userIds[uk2] = uid2; }
            nodeLines.push(uid2 + '(["' + mermaidLabel(users[u2]) + '"])');
            var chU2 = !!changedUserKeys[uk2];
            if (chU2) hlNodes[uid2] = true;
            edges.push({ from: iid, to: uid2, changed: chU2 });
          }
        }
      }
    }

    if (!nodeLines.length) return 'graph LR\n  empty["无商品数据"]';

    var lines = ['graph LR'].concat(nodeLines);
    var hlEdges = [];
    for (var e = 0; e < edges.length; e++) {
      lines.push(edges[e].from + ' --> ' + edges[e].to);
      if (edges[e].changed) hlEdges.push(e);
    }
    lines.push('classDef hlNode fill:#ffe08a,stroke:#d97706,stroke-width:3px,color:#7c2d12;');
    var hlIds = Object.keys(hlNodes);
    if (hlIds.length) lines.push('class ' + hlIds.join(',') + ' hlNode;');
    if (hlEdges.length) lines.push('linkStyle ' + hlEdges.join(',') + ' stroke:#dc2626,stroke-width:3px;');
    return lines.join('\n');
  }

  function renderMermaid() {
    var host = $('mermaid-view');
    if (!window.mermaid) {
      host.innerHTML = '<div class="mermaid-fallback">Mermaid CDN 不可用（离线或网络受限）。' +
        '表格视图仍可正常使用。</div>';
      return;
    }
    if (!state.mermaidReady) {
      try { window.mermaid.initialize({ startOnLoad: false, securityLevel: 'loose', theme: 'default' }); }
      catch (e) { /* ignore */ }
      state.mermaidReady = true;
    }
    var def = buildMermaid();
    var rid = 'mermaid-' + Date.now() + '-' + Math.floor(Math.random() * 1000);
    host.innerHTML = '<div class="hint">渲染中…</div>';
    try {
      window.mermaid.render(rid, def).then(function (res) {
        host.innerHTML = res.svg;
      }).catch(function (err) {
        host.innerHTML = '<div class="mermaid-fallback">Mermaid 渲染失败：' +
          esc(err && err.message ? err.message : err) + '</div><pre class="mermaid-src">' + esc(def) + '</pre>';
      });
    } catch (err) {
      host.innerHTML = '<div class="mermaid-fallback">Mermaid 渲染异常：' +
        esc(err && err.message ? err.message : err) + '</div><pre class="mermaid-src">' + esc(def) + '</pre>';
    }
  }

  function renderColumn() {
    var root = $('column-view');
    var steps = state.data.steps || [];
    var items = state.data.items || [];

    var rows = [];
    for (var i = 0; i < items.length; i++) {
      var it = items[i];
      if (it.variants && it.variants.length) {
        for (var v = 0; v < it.variants.length; v++) {
          rows.push({ key: boardKey(it.item_id, it.variants[v].variant_id), label: it.name + ' / ' + it.variants[v].name });
        }
      } else {
        rows.push({ key: boardKey(it.item_id, ''), label: it.name });
      }
    }

    var stepCells = [];
    for (var j = 0; j < steps.length; j++) {
      var map = {};
      var ch = steps[j].changed || [];
      for (var c = 0; c < ch.length; c++) {
        if (!ch[c].after) continue;
        map[boardKey(ch[c].item_id, ch[c].variant_id)] = ch[c].after;
      }
      stepCells.push(map);
    }

    var html = '<div class="col-wrap"><table class="col-table"><thead><tr>' +
      '<th class="rowhead">商品 / 变体</th>';
    for (var j2 = 0; j2 < steps.length; j2++) {
      var msg = msgBySeq(steps[j2].message_seq);
      var disp = msg ? (msg.display || msg.user) : ('#' + steps[j2].message_seq);
      var cur = (j2 === state.current) ? ' current' : '';
      html += '<th class="colhead' + cur + '" data-idx="' + j2 + '" title="' + esc(disp) + '：' +
        esc(msg ? msg.text : '') + '">#' + esc(steps[j2].message_seq) +
        '<br><span>' + esc(disp) + '</span></th>';
    }
    html += '</tr></thead><tbody>';
    for (var r = 0; r < rows.length; r++) {
      html += '<tr><th class="rowhead">' + esc(rows[r].label) + '</th>';
      for (var j3 = 0; j3 < steps.length; j3++) {
        var user = stepCells[j3][rows[r].key];
        var cls = 'colcell' + (j3 === state.current ? ' current' : '') + (user ? ' filled' : '');
        html += '<td class="' + cls + '" data-idx="' + j3 + '">' + (user ? esc(user) : '') + '</td>';
      }
      html += '</tr>';
    }
    html += '</tbody></table></div>';
    root.innerHTML = html;

    function bind(nodes) {
      for (var n = 0; n < nodes.length; n++) {
        nodes[n].addEventListener('click', function () {
          setPlaying(false);
          goToStep(parseInt(this.getAttribute('data-idx'), 10));
        });
      }
    }
    bind(root.querySelectorAll('th.colhead'));
    bind(root.querySelectorAll('td.colcell'));
    var curHead = root.querySelector('th.colhead.current');
    if (curHead && curHead.scrollIntoView) curHead.scrollIntoView({ block: 'nearest', inline: 'center' });
  }

  function renderWhoWhats() {
    var panel = $('who-whats-panel');
    var list = P.normalizeWhoWhats({ who_whats: (state.data && state.data.who_whats) || [] });
    if (!list.length) { panel.classList.add('hidden'); return; }
    panel.classList.remove('hidden');
    $('ww-count').textContent = list.length + ' 人';
    $('ww-title').textContent = (state.wwOpen ? '▾ ' : '▸ ') + 'Who-Whats';
    $('ww-body').classList.toggle('hidden', !state.wwOpen);
    P.syncKeyedChildren(
      $('who-whats-body'),
      list,
      function (w) { return w.display; },
      function () {
        var tr = document.createElement('tr');
        tr.innerHTML = '<td class="ww-display"></td><td class="ww-identity"></td><td class="ww-items"></td>';
        tr.__d = tr.querySelector('.ww-display');
        tr.__i = tr.querySelector('.ww-identity');
        tr.__it = tr.querySelector('.ww-items');
        return tr;
      },
      function (tr, w) {
        if (tr.__d.textContent !== w.display) tr.__d.textContent = w.display;
        if (tr.__i.textContent !== w.identity) tr.__i.textContent = w.identity;
        var detail = w.items.map(function (it) { return it.name + ' ×' + it.qty; }).join('、');
        if (tr.__it.textContent !== detail) tr.__it.textContent = detail;
      }
    );
  }

  function keyLabel(key) {
    var parts = key.split('|');
    var itemId = parts[0];
    var variantId = parts.slice(1).join('|');
    var name = itemId;
    var items = (state.data && state.data.items) || [];
    for (var i = 0; i < items.length; i++) {
      if (items[i].item_id === itemId) { name = items[i].name; break; }
    }
    return variantId ? name + ' / ' + variantId : name + ' / 单领';
  }

  function renderCheck() {
    var bar = $('check-bar');
    var expected = state.data && state.data.expected;
    if (!expected || !Object.keys(expected).length) {
      bar.classList.add('hidden');
      bar.innerHTML = '';
      return;
    }
    bar.classList.remove('hidden');
    var board = state.cumulative.length ? (state.cumulative[state.cumulative.length - 1] || {}) : {};
    var diffs = [];
    var keys = Object.keys(expected);
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      var exp = expected[key] || [];
      var slots = (board[key] || []).slice().sort(function (a, b) { return a.slot - b.slot; });
      var actual = slots
        .filter(function (s) { return s.user && s.status !== 'empty'; })
        .map(function (s) { return s.user; });
      var same = exp.length === actual.length;
      if (same) {
        for (var j = 0; j < exp.length; j++) {
          if (exp[j] !== actual[j]) { same = false; break; }
        }
      }
      if (!same) diffs.push({ key: key, exp: exp, actual: actual });
    }

    if (!diffs.length) {
      bar.innerHTML = '<span class="pass">PASS</span>期望与当前最终态一致（共 ' +
        keys.length + ' 个分组）。';
      return;
    }

    var html = '<span class="fail">FAIL</span>期望与当前最终态存在 ' + diffs.length + ' 处差异：<ul class="diff-list">';
    for (var d = 0; d < diffs.length; d++) {
      html += '<li><span class="k">' + esc(keyLabel(diffs[d].key)) + '</span><br>' +
        '<span class="exp">期望：' + esc(diffs[d].exp.join(' → ') || '（空）') + '</span><br>' +
        '<span class="act">实际：' + esc(diffs[d].actual.join(' → ') || '（空）') + '</span></li>';
    }
    html += '</ul>';
    bar.innerHTML = html;
  }

  function renderRight() {
    $('table-view').classList.toggle('hidden', state.view !== 'table');
    $('column-view').classList.toggle('hidden', state.view !== 'column');
    $('mermaid-view').classList.toggle('hidden', state.view !== 'mermaid');
    if (state.view === 'table') renderBoard();
    else if (state.view === 'column') renderColumn();
    else renderMermaid();
  }

  function renderAll() {
    renderHeader();
    renderList();
    renderRight();
  }

  function goToStep(idx, opts) {
    if (!state.data) return;
    var steps = state.data.steps || [];
    if (!steps.length) return;
    idx = Math.max(0, Math.min(steps.length - 1, idx));
    state.current = idx;
    if (!opts || !opts.keepFocus) state.focusedKey = null;
    renderAll();
    updateControls();
  }

  function onCellClick(key) {
    var idx = state.lastChange[key];
    if (idx == null) {
      P.toast('该格在本次重放中未被修改');
      return;
    }
    state.focusedKey = key;
    setPlaying(false);
    goToStep(idx, { keepFocus: true });
    P.toast('已跳到第 ' + (idx + 1) + ' 步：最后一次修改该格的消息', 'ok');
  }

  function setPlaying(p) {
    state.playing = p;
    if (state.timer) { clearTimeout(state.timer); state.timer = null; }
    updateControls();
    if (p) scheduleTick();
  }

  function scheduleTick() {
    if (!state.playing) return;
    var delay = Math.max(120, 1000 / state.speed);
    state.timer = setTimeout(function () {
      if (!state.playing) return;
      var steps = state.data ? (state.data.steps || []) : [];
      if (state.current >= steps.length - 1) { setPlaying(false); return; }
      goToStep(state.current + 1);
      scheduleTick();
    }, delay);
  }

  function updateControls() {
    var steps = state.data ? (state.data.steps || []) : [];
    var has = steps.length > 0;
    var atStart = !has || state.current <= 0;
    var atEnd = !has || state.current >= steps.length - 1;
    $('btn-first').disabled = atStart;
    $('btn-prev').disabled = atStart;
    $('btn-next').disabled = atEnd;
    $('btn-last').disabled = atEnd;
    $('btn-play').disabled = !has;
    $('btn-play').textContent = state.playing ? '⏸ 暂停' : '▶ 播放';
  }

  function isValid(json) {
    return json && typeof json === 'object' &&
      Array.isArray(json.items) && Array.isArray(json.steps);
  }

  function loadData(json, source) {
    if (!isValid(json)) {
      P.toast('数据格式不完整：至少需要 items 与 steps 数组', 'bad');
      return;
    }
    if (!Array.isArray(json.messages)) json.messages = [];
    state.data = json;
    state.source = source || '';
    state.meta = P.buildMeta({ round: { items: json.items } });
    state.current = 0;
    state.focusedKey = null;
    setPlaying(false);
    buildDerived();

    $('empty-hint').classList.add('hidden');
    $('replay-main').classList.remove('hidden');
    renderWhoWhats();
    renderCheck();
    renderAll();
    updateControls();
    if (state.view === 'mermaid') renderMermaid();
  }

  function loadEmbeddedFallback(reason) {
    loadData(EMBEDDED_SAMPLE, '内置占位示例' + (reason ? '（' + reason + '）' : ''));
  }

  function loadDefault() {
    if (!window.fetch) { loadEmbeddedFallback('无 fetch'); return; }
    fetch('replay_view.json', { cache: 'no-store' }).then(function (res) {
      if (!res.ok) throw new Error('HTTP ' + res.status);
      return res.json();
    }).then(function (json) {
      loadData(json, 'replay_view.json');
    }).catch(function () {
      loadEmbeddedFallback('读取失败，已回退');
    });
  }

  function bindFileInput() {
    $('file-input').addEventListener('change', function (ev) {
      var file = ev.target.files && ev.target.files[0];
      if (!file) return;
      var reader = new FileReader();
      reader.onload = function () {
        try {
          loadData(JSON.parse(String(reader.result)), file.name);
          P.toast('已载入 ' + file.name, 'ok');
        } catch (e) {
          P.toast('JSON 解析失败：' + (e && e.message ? e.message : e), 'bad');
        }
      };
      reader.onerror = function () { P.toast('文件读取失败', 'bad'); };
      reader.readAsText(file, 'utf-8');
      ev.target.value = '';
    });
  }

  function switchView(view) {
    state.view = view;
    $('tab-table').classList.toggle('active', view === 'table');
    $('tab-column').classList.toggle('active', view === 'column');
    $('tab-mermaid').classList.toggle('active', view === 'mermaid');
    renderRight();
  }

  function init() {
    tableEl = $('table-view');
    boardRenderer = P.createBoardRenderer(tableEl, { persistent: true });

    tableEl.addEventListener('click', function (ev) {
      var cell = ev.target && ev.target.closest ? ev.target.closest('.cell') : null;
      if (!cell || !cell.__k) return;
      onCellClick(cell.__k);
    });

    $('btn-first').addEventListener('click', function () { setPlaying(false); goToStep(0); });
    $('btn-prev').addEventListener('click', function () { setPlaying(false); goToStep(state.current - 1); });
    $('btn-next').addEventListener('click', function () { setPlaying(false); goToStep(state.current + 1); });
    $('btn-last').addEventListener('click', function () {
      setPlaying(false);
      goToStep((state.data && state.data.steps ? state.data.steps.length : 0) - 1);
    });
    $('btn-play').addEventListener('click', function () { setPlaying(!state.playing); });
    $('speed').addEventListener('change', function (e) { state.speed = parseFloat(e.target.value) || 1; });

    $('tab-table').addEventListener('click', function () { switchView('table'); });
    $('tab-column').addEventListener('click', function () { switchView('column'); });
    $('tab-mermaid').addEventListener('click', function () { switchView('mermaid'); });

    $('ww-toggle').addEventListener('click', function () {
      state.wwOpen = !state.wwOpen;
      renderWhoWhats();
    });

    document.addEventListener('keydown', function (e) {
      var tag = e.target && e.target.tagName;
      if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
      if (!state.data) return;
      var steps = state.data.steps || [];
      if (e.key === 'ArrowRight') { e.preventDefault(); setPlaying(false); goToStep(state.current + 1); }
      else if (e.key === 'ArrowLeft') { e.preventDefault(); setPlaying(false); goToStep(state.current - 1); }
      else if (e.key === ' ') { e.preventDefault(); setPlaying(!state.playing); }
      else if (e.key === 'Home') { e.preventDefault(); setPlaying(false); goToStep(0); }
      else if (e.key === 'End') { e.preventDefault(); setPlaying(false); goToStep(steps.length - 1); }
    });

    bindFileInput();
    updateControls();
    loadDefault();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
