(function () {
  'use strict';

  /* ------------------------------------------------------------------ *
   * 内嵌示例数据（与 sample_replay.json 等价）
   * 目的：file:// 下 fetch 本地 json 会被浏览器拦截，必须有内嵌回退。
   * ------------------------------------------------------------------ */
  var EMBEDDED_SAMPLE = {
    sample_id: '月行水上',
    title: '月行水上',
    items: [
      {
        item_id: '月行水上/特典卡组-校园凭证',
        name: '特典卡组-校园凭证',
        kind: 'split',
        section: '特典卡组-校园凭证',
        variants: [{ variant_id: '整套', name: '整套', capacity: 5 }]
      },
      {
        item_id: '月行水上/通行证SP-月行水上',
        name: '通行证SP-月行水上',
        kind: 'single',
        section: '单领',
        variants: []
      },
      {
        item_id: '月行水上/徽章-月行水上',
        name: '徽章-月行水上',
        kind: 'split',
        section: '徽章-月行水上',
        variants: [
          { variant_id: '亚克力', name: '亚克力', capacity: 3 },
          { variant_id: '吧唧', name: '吧唧', capacity: 4 }
        ]
      }
    ],
    messages: [
      { seq: 1, pos: 1, user: 'SIM', display: 'SIM', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1', claims: [{ item_id: '月行水上/特典卡组-校园凭证', item_name: '特典卡组-校园凭证', variant_id: '整套', variant_name: '整套', qty: 1 }] },
      { seq: 2, pos: 2, user: '嘟嘟', display: '嘟嘟', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1', claims: [{ item_id: '月行水上/特典卡组-校园凭证', item_name: '特典卡组-校园凭证', variant_id: '整套', variant_name: '整套', qty: 1 }] },
      { seq: 3, pos: 3, user: 'DKLA', display: 'DKLA', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1', claims: [{ item_id: '月行水上/特典卡组-校园凭证', item_name: '特典卡组-校园凭证', variant_id: '整套', variant_name: '整套', qty: 1 }] },
      { seq: 4, pos: 4, user: 'cz', display: 'cz', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1', claims: [{ item_id: '月行水上/特典卡组-校园凭证', item_name: '特典卡组-校园凭证', variant_id: '整套', variant_name: '整套', qty: 1 }] },
      { seq: 5, pos: 5, user: '静边城', display: '静边城', text: '排整套', status: 'Applied', detail: 'claim: 特典卡组-校园凭证 整套 x1', claims: [{ item_id: '月行水上/特典卡组-校园凭证', item_name: '特典卡组-校园凭证', variant_id: '整套', variant_name: '整套', qty: 1 }] },
      { seq: 6, pos: 6, user: '嘟嘟', display: '嘟嘟', text: '排亚克力1', status: 'Applied', detail: 'claim: 徽章-月行水上 亚克力 x1', claims: [{ item_id: '月行水上/徽章-月行水上', item_name: '徽章-月行水上', variant_id: '亚克力', variant_name: '亚克力', qty: 1 }] },
      { seq: 7, pos: 7, user: 'SIM', display: 'SIM', text: '排亚克力1', status: 'Applied', detail: 'claim: 徽章-月行水上 亚克力 x1', claims: [{ item_id: '月行水上/徽章-月行水上', item_name: '徽章-月行水上', variant_id: '亚克力', variant_name: '亚克力', qty: 1 }] },
      { seq: 8, pos: 8, user: '静边城', display: '静边城', text: '排吧唧1', status: 'Applied', detail: 'claim: 徽章-月行水上 吧唧 x1', claims: [{ item_id: '月行水上/徽章-月行水上', item_name: '徽章-月行水上', variant_id: '吧唧', variant_name: '吧唧', qty: 1 }] },
      { seq: 9, pos: 9, user: 'DKLA', display: 'DKLA', text: '排通行证SP1', status: 'Applied', detail: 'claim: 通行证SP-月行水上 x1', claims: [{ item_id: '月行水上/通行证SP-月行水上', item_name: '通行证SP-月行水上', variant_id: '', variant_name: '单领', qty: 1 }] },
      { seq: 10, pos: 10, user: '嘟嘟', display: '嘟嘟', text: '撤整套1', status: 'Applied', detail: 'cancel: 特典卡组-校园凭证 整套 x1', claims: [] },
      { seq: 11, pos: 11, user: 'cz', display: 'cz', text: '排吧唧1', status: 'Applied', detail: 'claim: 徽章-月行水上 吧唧 x1', claims: [{ item_id: '月行水上/徽章-月行水上', item_name: '徽章-月行水上', variant_id: '吧唧', variant_name: '吧唧', qty: 1 }] },
      { seq: 12, pos: 12, user: 'cz', display: 'cz', text: '排通行证SP1', status: 'Applied', detail: 'claim: 通行证SP-月行水上 x1', claims: [{ item_id: '月行水上/通行证SP-月行水上', item_name: '通行证SP-月行水上', variant_id: '', variant_name: '单领', qty: 1 }] },
      { seq: 13, pos: 13, user: '管理员', display: '管理员', text: '/锁位 吧唧 3号位给岚', status: 'Applied', detail: 'admin: 包尾锁位 吧唧 slot3 -> 岚', claims: [] }
    ],
    steps: [
      { index: 0, message_seq: 1, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: 'SIM', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 1, before: null, after: 'SIM', reason: 'NewClaimFilled' }] },
      { index: 1, message_seq: 2, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: 'SIM', status: 'filled' }, { slot: 2, user: '嘟嘟', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 2, before: null, after: '嘟嘟', reason: 'NewClaimFilled' }] },
      { index: 2, message_seq: 3, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: 'SIM', status: 'filled' }, { slot: 2, user: '嘟嘟', status: 'filled' }, { slot: 3, user: 'DKLA', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 3, before: null, after: 'DKLA', reason: 'NewClaimFilled' }] },
      { index: 3, message_seq: 4, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: 'SIM', status: 'filled' }, { slot: 2, user: '嘟嘟', status: 'filled' }, { slot: 3, user: 'DKLA', status: 'filled' }, { slot: 4, user: 'cz', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 4, before: null, after: 'cz', reason: 'NewClaimFilled' }] },
      { index: 4, message_seq: 5, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: 'SIM', status: 'filled' }, { slot: 2, user: '嘟嘟', status: 'filled' }, { slot: 3, user: 'DKLA', status: 'filled' }, { slot: 4, user: 'cz', status: 'filled' }, { slot: 5, user: '静边城', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 5, before: null, after: '静边城', reason: 'NewClaimFilled' }] },
      { index: 5, message_seq: 6, board: { '月行水上/徽章-月行水上|亚克力': [{ slot: 1, user: '嘟嘟', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '亚克力', slot: 1, before: null, after: '嘟嘟', reason: 'NewClaimFilled' }] },
      { index: 6, message_seq: 7, board: { '月行水上/徽章-月行水上|亚克力': [{ slot: 1, user: '嘟嘟', status: 'filled' }, { slot: 2, user: 'SIM', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '亚克力', slot: 2, before: null, after: 'SIM', reason: 'NewClaimFilled' }] },
      { index: 7, message_seq: 8, board: { '月行水上/徽章-月行水上|吧唧': [{ slot: 1, user: '静边城', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '吧唧', slot: 1, before: null, after: '静边城', reason: 'NewClaimFilled' }] },
      { index: 8, message_seq: 9, board: { '月行水上/通行证SP-月行水上|': [{ slot: 1, user: 'DKLA', status: 'filled' }] }, changed: [{ item_id: '月行水上/通行证SP-月行水上', variant_id: '', slot: 1, before: null, after: 'DKLA', reason: 'NewClaimFilled' }] },
      { index: 9, message_seq: 10, board: { '月行水上/特典卡组-校园凭证|整套': [{ slot: 1, user: 'SIM', status: 'filled' }, { slot: 2, user: 'DKLA', status: 'filled' }, { slot: 3, user: 'cz', status: 'filled' }, { slot: 4, user: '静边城', status: 'filled' }] }, changed: [{ item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 2, before: '嘟嘟', after: 'DKLA', reason: 'AutoMovedForward' }, { item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 3, before: 'DKLA', after: 'cz', reason: 'AutoMovedForward' }, { item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 4, before: 'cz', after: '静边城', reason: 'AutoMovedForward' }, { item_id: '月行水上/特典卡组-校园凭证', variant_id: '整套', slot: 5, before: '静边城', after: null, reason: 'CancelReleased' }] },
      { index: 10, message_seq: 11, board: { '月行水上/徽章-月行水上|吧唧': [{ slot: 1, user: '静边城', status: 'filled' }, { slot: 2, user: 'cz', status: 'filled' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '吧唧', slot: 2, before: null, after: 'cz', reason: 'NewClaimFilled' }] },
      { index: 11, message_seq: 12, board: { '月行水上/通行证SP-月行水上|': [{ slot: 1, user: 'DKLA', status: 'filled' }, { slot: 2, user: 'cz', status: 'filled' }] }, changed: [{ item_id: '月行水上/通行证SP-月行水上', variant_id: '', slot: 2, before: null, after: 'cz', reason: 'NewClaimFilled' }] },
      { index: 12, message_seq: 13, board: { '月行水上/徽章-月行水上|吧唧': [{ slot: 1, user: '静边城', status: 'filled' }, { slot: 2, user: 'cz', status: 'filled' }, { slot: 3, user: '岚', status: 'locked' }] }, changed: [{ item_id: '月行水上/徽章-月行水上', variant_id: '吧唧', slot: 3, before: null, after: '岚', reason: 'TailSegmentCreated' }] }
    ],
    who_whats: [
      { display: 'SIM', identity: 'SIM（购物金）', items: [{ name: '整套', qty: 1 }, { name: '亚克力', qty: 1 }] },
      { display: '嘟嘟', identity: '嘟嘟', items: [{ name: '亚克力', qty: 1 }] },
      { display: 'DKLA', identity: 'DKLA', items: [{ name: '整套', qty: 1 }, { name: '通行证SP', qty: 1 }] },
      { display: 'cz', identity: 'cz', items: [{ name: '整套', qty: 1 }, { name: '吧唧', qty: 1 }, { name: '通行证SP', qty: 1 }] },
      { display: '静边城', identity: '静边城', items: [{ name: '整套', qty: 1 }, { name: '吧唧', qty: 1 }] },
      { display: '岚', identity: '岚（包尾锁位）', items: [{ name: '吧唧', qty: 1 }] }
    ],
    expected: {
      '月行水上/特典卡组-校园凭证|整套': ['SIM', 'DKLA', 'cz', '静边城'],
      '月行水上/徽章-月行水上|亚克力': ['嘟嘟', 'SIM'],
      '月行水上/徽章-月行水上|吧唧': ['静边城', 'cz', '岚'],
      '月行水上/通行证SP-月行水上|': ['DKLA', 'cz']
    }
  };

  var REASON_META = {
    NewClaimFilled: { cls: 'chg-new', icon: '▲', label: '新增填入' },
    CancelReleased: { cls: 'chg-release', icon: '▼', label: '撤销释放' },
    AutoMovedForward: { cls: 'chg-forward', icon: '↷', label: '自动前移' },
    TailSegmentCreated: { cls: 'chg-tail', icon: '⛓', label: '包尾段创建' },
    TailSegmentUpdated: { cls: 'chg-tail', icon: '⛓', label: '包尾段更新' },
    AdminFixed: { cls: 'chg-admin', icon: '✚', label: '管理员固定' },
    AdminUnlocked: { cls: 'chg-admin', icon: '✚', label: '管理员解锁' },
    RecomputedByRuleChange: { cls: 'chg-forward', icon: '↻', label: '规则重算' }
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
    maxSlot: {},
    lastChange: {},
    focusedCell: null,
    mermaidReady: false
  };

  function el(sel) { return document.querySelector(sel); }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function boardKey(itemId, variantId) {
    return itemId + '|' + (variantId == null ? '' : variantId);
  }

  function kindLabel(k) { return KIND_LABEL[k] || k || ''; }

  function msgBySeq(seq) {
    if (!state.data) return null;
    var msgs = state.data.messages || [];
    for (var i = 0; i < msgs.length; i++) {
      if (msgs[i].seq === seq) return msgs[i];
    }
    return null;
  }

  function changesByKeySlot(step) {
    var map = {};
    if (!step || !step.changed) return map;
    for (var i = 0; i < step.changed.length; i++) {
      var c = step.changed[i];
      var key = boardKey(c.item_id, c.variant_id);
      if (!map[key]) map[key] = {};
      map[key][c.slot] = c;
    }
    return map;
  }

  function buildLastChange(steps) {
    var m = {};
    for (var i = 0; i < steps.length; i++) {
      var ch = steps[i].changed || [];
      for (var j = 0; j < ch.length; j++) {
        var c = ch[j];
        m[boardKey(c.item_id, c.variant_id) + '#' + c.slot] = i;
      }
    }
    return m;
  }

  function buildDerived() {
    var steps = state.data.steps || [];
    var boards = [];
    var cur = {};
    var maxSlot = {};
    for (var i = 0; i < steps.length; i++) {
      var next = {};
      for (var k in cur) { if (Object.prototype.hasOwnProperty.call(cur, k)) next[k] = cur[k].slice(); }
      var b = steps[i].board || {};
      for (var k2 in b) {
        if (!Object.prototype.hasOwnProperty.call(b, k2)) continue;
        next[k2] = b[k2].slice();
        for (var s = 0; s < b[k2].length; s++) {
          var slot = b[k2][s].slot;
          if (!maxSlot[k2] || slot > maxSlot[k2]) maxSlot[k2] = slot;
        }
      }
      boards.push(next);
      cur = next;
    }
    state.cumulative = boards;
    state.maxSlot = maxSlot;
    state.lastChange = buildLastChange(steps);
  }

  /* ------------------------------------------------------------------ *
   * 渲染：顶部
   * ------------------------------------------------------------------ */
  function renderHeader() {
    if (!state.data) return;
    var steps = state.data.steps || [];
    el('#title').textContent = state.data.title || state.data.sample_id || '排谷重放';
    el('#step-info').textContent =
      '共 ' + steps.length + ' 步 · 当前第 ' + (steps.length ? state.current + 1 : 0) + ' 步';
    el('#step-count').textContent = steps.length ? (state.current + 1) + '/' + steps.length : '0/0';
    el('#source-info').textContent = state.source ? '数据源：' + state.source : '';
  }

  /* ------------------------------------------------------------------ *
   * 渲染：左侧消息列表
   * ------------------------------------------------------------------ */
  function renderList() {
    var ul = el('#step-list');
    ul.innerHTML = '';
    var steps = state.data.steps || [];
    for (var i = 0; i < steps.length; i++) {
      var step = steps[i];
      var msg = msgBySeq(step.message_seq);
      var li = document.createElement('li');
      li.className = 'step-item' + (i === state.current ? ' current' : '');
      li.setAttribute('data-idx', i);
      var seq = msg ? msg.seq : step.message_seq;
      var display = msg ? (msg.display || msg.user) : '?';
      var text = msg ? msg.text : '';
      var status = msg ? (msg.status || '') : '';
      var stCls = status ? ' st-' + String(status).toLowerCase() : '';
      li.innerHTML =
        '<span class="seq">#' + esc(seq) + '</span>' +
        '<span class="who">' + esc(display) + '</span>' +
        '<span class="txt" title="' + esc(text) + '">' + esc(text) + '</span>' +
        '<span class="status' + stCls + '">' + esc(status) + '</span>';
      li.addEventListener('click', function (idx) {
        return function () { setPlaying(false); goToStep(idx); };
      }(i));
      ul.appendChild(li);
    }
    var curEl = ul.querySelector('.current');
    if (curEl && curEl.scrollIntoView) curEl.scrollIntoView({ block: 'nearest' });
  }

  /* ------------------------------------------------------------------ *
   * 渲染：表格视图
   * ------------------------------------------------------------------ */
  function renderTable() {
    var root = el('#table-view');
    root.innerHTML = '';
    var step = (state.data.steps || [])[state.current];
    var board = state.cumulative[state.current] || {};
    var changes = changesByKeySlot(step);

    var items = state.data.items || [];
    var sectionOrder = [];
    var bySection = {};
    for (var i = 0; i < items.length; i++) {
      var sec = items[i].section || '未分组';
      if (!bySection[sec]) { bySection[sec] = []; sectionOrder.push(sec); }
      bySection[sec].push(items[i]);
    }

    for (var s = 0; s < sectionOrder.length; s++) {
      var secName = sectionOrder[s];
      var secEl = document.createElement('section');
      secEl.className = 'section';
      var h = document.createElement('h3');
      h.className = 'section-title';
      h.textContent = secName;
      secEl.appendChild(h);
      var secItems = bySection[secName];
      for (var j = 0; j < secItems.length; j++) {
        secEl.appendChild(renderItem(secItems[j], board, changes));
      }
      root.appendChild(secEl);
    }
  }

  function renderItem(item, board, changes) {
    var wrap = document.createElement('div');
    wrap.className = 'item';
    var head = document.createElement('div');
    head.className = 'item-head';
    head.innerHTML =
      '<span class="item-name">' + esc(item.name) + '</span>' +
      '<span class="kind-tag kind-' + esc(item.kind) + '">' + esc(kindLabel(item.kind)) + '</span>';
    wrap.appendChild(head);

    var variants = (item.variants && item.variants.length)
      ? item.variants
      : [{ variant_id: '', name: (item.kind === 'single' ? '单领' : '默认'), capacity: 0 }];

    var grid = document.createElement('div');
    grid.className = 'variant-grid';
    for (var i = 0; i < variants.length; i++) {
      grid.appendChild(renderVariantRow(item, variants[i], board, changes));
    }
    wrap.appendChild(grid);
    return wrap;
  }

  function renderVariantRow(item, variant, board, changes) {
    var key = boardKey(item.item_id, variant.variant_id);
    var slots = board[key] || [];
    var capacity = variant.capacity || 0;
    var maxSlot = Math.max(capacity, state.maxSlot[key] || 0, 1);
    var bySlot = {};
    for (var i = 0; i < slots.length; i++) bySlot[slots[i].slot] = slots[i];
    var chg = changes[key] || {};

    var row = document.createElement('div');
    row.className = 'variant-row';

    var label = document.createElement('div');
    label.className = 'variant-label';
    label.title = variant.name + (capacity ? ' 容量 ' + capacity : '');
    label.textContent = variant.name + (capacity ? '（' + capacity + '）' : '');
    row.appendChild(label);

    var cells = document.createElement('div');
    cells.className = 'cells';

    for (var s = 1; s <= maxSlot; s++) {
      var slotObj = bySlot[s];
      var cell = document.createElement('div');
      cell.className = 'cell';
      cell.setAttribute('data-key', key);
      cell.setAttribute('data-slot', s);
      cell.setAttribute('data-slot-num', s);

      var occupied = slotObj && slotObj.status && slotObj.status !== 'empty' && slotObj.user;
      if (occupied) {
        cell.classList.add('filled');
        cell.textContent = slotObj.user;
        if (slotObj.status === 'locked') cell.classList.add('locked');
      } else {
        cell.classList.add('empty');
      }

      var change = chg[s];
      if (change) {
        var meta = REASON_META[change.reason] || REASON_META.NewClaimFilled;
        cell.classList.add('changed', meta.cls);
        var badge = document.createElement('span');
        badge.className = 'badge';
        badge.textContent = meta.icon;
        cell.appendChild(badge);
        cell.title = meta.label + '：' + (change.before == null ? '空' : change.before) +
          ' → ' + (change.after == null ? '空' : change.after);
      } else if (occupied) {
        cell.title = slotObj.user;
      }

      if (state.focusedCell && state.focusedCell.key === key && state.focusedCell.slot === s) {
        cell.classList.add('focused');
      }

      cell.addEventListener('click', function (k, sn) {
        return function () { onCellClick(k, sn); };
      }(key, s));

      cells.appendChild(cell);
    }

    row.appendChild(cells);
    return row;
  }

  /* ------------------------------------------------------------------ *
   * 渲染：Mermaid 视图
   * ------------------------------------------------------------------ */
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
          for (var uu2 = 0; uu2 < users.length; uu2++) {
            var uk2 = key + '||' + users[uu2];
            var uid2 = userIds[uk2];
            if (!uid2) { uid2 = nextId(); userIds[uk2] = uid2; }
            nodeLines.push(uid2 + '(["' + mermaidLabel(users[uu2]) + '"])');
            var chU2 = !!changedUserKeys[uk2];
            if (chU2) hlNodes[uid2] = true;
            edges.push({ from: iid, to: uid2, changed: chU2 });
          }
        }
      }
    }

    if (!nodeLines.length) {
      return 'graph LR\n  empty["无商品数据"]';
    }

    var lines = ['graph LR'];
    lines = lines.concat(nodeLines);
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
    var host = el('#mermaid-view');
    if (!window.mermaid) {
      host.innerHTML = '<div class="fallback">Mermaid CDN 不可用（离线或网络受限）。' +
        '表格视图仍可正常使用，可点击左上「表格视图」切换。</div>';
      return;
    }
    if (!state.mermaidReady) {
      try {
        window.mermaid.initialize({ startOnLoad: false, securityLevel: 'loose', theme: 'default' });
      } catch (e) { /* ignore */ }
      state.mermaidReady = true;
    }
    var def = buildMermaid();
    var rid = 'mermaid-' + Date.now() + '-' + Math.floor(Math.random() * 1000);
    host.innerHTML = '<div class="mermaid-pending">渲染中…</div>';
    try {
      window.mermaid.render(rid, def).then(function (res) {
        host.innerHTML = res.svg;
      }).catch(function (err) {
        host.innerHTML = '<div class="fallback">Mermaid 渲染失败：' +
          esc(err && err.message ? err.message : err) + '</div><pre class="mermaid-src">' + esc(def) + '</pre>';
      });
    } catch (err) {
      host.innerHTML = '<div class="fallback">Mermaid 渲染异常：' +
        esc(err && err.message ? err.message : err) + '</div><pre class="mermaid-src">' + esc(def) + '</pre>';
    }
  }

  function renderRight() {
    var tableView = el('#table-view');
    var columnView = el('#column-view');
    var mermaidView = el('#mermaid-view');
    tableView.classList.add('hidden');
    columnView.classList.add('hidden');
    mermaidView.classList.add('hidden');
    if (state.view === 'column') {
      columnView.classList.remove('hidden');
      renderColumn();
    } else if (state.view === 'table') {
      tableView.classList.remove('hidden');
      renderTable();
    } else {
      mermaidView.classList.remove('hidden');
      renderMermaid();
    }
  }

  /* ------------------------------------------------------------------ *
   * 渲染：列视图（列 = 认购顺序/消息，行 = 商品/变体）
   * ------------------------------------------------------------------ */
  function renderColumn() {
    var root = el('#column-view');
    root.innerHTML = '';
    var steps = state.data.steps || [];
    var items = state.data.items || [];

    var rows = [];
    for (var i = 0; i < items.length; i++) {
      var it = items[i];
      if (it.variants && it.variants.length) {
        for (var v = 0; v < it.variants.length; v++) {
          rows.push({ key: boardKey(it.item_id, it.variants[v].variant_id),
                      label: it.name + ' / ' + it.variants[v].name });
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
        var chg = ch[c];
        if (!chg.after) continue;
        map[boardKey(chg.item_id, chg.variant_id)] = chg.after;
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

    var heads = root.querySelectorAll('th.colhead');
    for (var h = 0; h < heads.length; h++) {
      heads[h].addEventListener('click', function () {
        setPlaying(false); goToStep(parseInt(this.getAttribute('data-idx'), 10));
      });
    }
    var cells = root.querySelectorAll('td.colcell');
    for (var q = 0; q < cells.length; q++) {
      cells[q].addEventListener('click', function () {
        setPlaying(false); goToStep(parseInt(this.getAttribute('data-idx'), 10));
      });
    }
    var curHead = root.querySelector('th.colhead.current');
    if (curHead && curHead.scrollIntoView) curHead.scrollIntoView({ block: 'nearest', inline: 'center' });
  }

  /* ------------------------------------------------------------------ *
   * 渲染：who-whats
   * ------------------------------------------------------------------ */
  function renderWhoWhats() {
    var host = el('#who-whats');
    var list = state.data.who_whats || [];
    if (!list.length) { host.classList.add('hidden'); host.innerHTML = ''; return; }
    host.classList.remove('hidden');
    var rows = '';
    for (var i = 0; i < list.length; i++) {
      var w = list[i];
      var detail = (w.items || []).map(function (it) {
        return esc(it.name) + ' ×' + esc(it.qty);
      }).join('、');
      rows += '<tr><td>' + esc(w.display) + '</td><td>' + esc(w.identity) + '</td><td>' + detail + '</td></tr>';
    }
    host.innerHTML =
      '<div class="ww-head" id="ww-toggle">▸ Who-Whats（' + list.length + ' 人）</div>' +
      '<div class="ww-body hidden" id="ww-body">' +
      '<table><thead><tr><th>display</th><th>身份</th><th>明细</th></tr></thead><tbody>' +
      rows + '</tbody></table></div>';
    el('#ww-toggle').addEventListener('click', function () {
      var body = el('#ww-body');
      var head = el('#ww-toggle');
      var collapsed = body.classList.toggle('hidden');
      head.textContent = (collapsed ? '▸ ' : '▾ ') + 'Who-Whats（' + list.length + ' 人）';
    });
  }

  /* ------------------------------------------------------------------ *
   * 渲染：核对条（expected vs 最终态）
   * ------------------------------------------------------------------ */
  function finalBoard() {
    if (!state.cumulative.length) return {};
    return state.cumulative[state.cumulative.length - 1] || {};
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
    var bar = el('#check-bar');
    var expected = state.data.expected;
    if (!expected || !Object.keys(expected).length) {
      bar.classList.add('hidden');
      bar.innerHTML = '';
      return;
    }
    bar.classList.remove('hidden');
    var board = finalBoard();
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

  /* ------------------------------------------------------------------ *
   * 交互
   * ------------------------------------------------------------------ */
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
    if (!opts || !opts.keepFocus) state.focusedCell = null;
    renderAll();
    updateControls();
  }

  function onCellClick(key, slot) {
    var idx = state.lastChange[key + '#' + slot];
    if (idx == null) {
      showToast('该格在本次重放中未被修改');
      return;
    }
    state.focusedCell = { key: key, slot: slot };
    setPlaying(false);
    goToStep(idx, { keepFocus: true });
    showToast('已跳到第 ' + (idx + 1) + ' 步：最后一次修改该格的消息');
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
    el('#btn-first').disabled = atStart;
    el('#btn-prev').disabled = atStart;
    el('#btn-next').disabled = atEnd;
    el('#btn-last').disabled = atEnd;
    el('#btn-play').disabled = !has;
    el('#btn-play').textContent = state.playing ? '⏸ 暂停' : '▶ 播放';
  }

  var toastTimer = null;
  function showToast(msg) {
    var t = el('#toast');
    t.textContent = msg;
    t.classList.add('show');
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.classList.remove('show'); }, 1800);
  }

  /* ------------------------------------------------------------------ *
   * 数据加载
   * ------------------------------------------------------------------ */
  function isValid(json) {
    return json && typeof json === 'object' &&
      Array.isArray(json.items) && Array.isArray(json.steps);
  }

  function loadData(json, source) {
    if (!isValid(json)) {
      showToast('数据格式不完整：至少需要 items 与 steps 数组');
      return;
    }
    if (!Array.isArray(json.messages)) json.messages = [];
    state.data = json;
    state.source = source || '';
    state.current = 0;
    state.focusedCell = null;
    setPlaying(false);
    buildDerived();

    el('#empty-hint').classList.add('hidden');
    el('#main').classList.remove('hidden');
    renderWhoWhats();
    renderCheck();
    renderAll();
    updateControls();

    if (state.view === 'mermaid') renderMermaid();
  }

  function loadEmbeddedFallback(reason) {
    loadData(EMBEDDED_SAMPLE, '内置示例 sample_replay.json' + (reason ? '（' + reason + '）' : ''));
  }

  function loadDefault() {
    var url = 'replay_view.json';
    if (!window.fetch) { loadEmbeddedFallback('无 fetch'); return; }
    fetch(url, { cache: 'no-store' }).then(function (res) {
      if (!res.ok) throw new Error('HTTP ' + res.status);
      return res.json();
    }).then(function (json) {
      loadData(json, 'replay_view.json');
    }).catch(function (err) {
      loadEmbeddedFallback('file:// 或读取失败，已回退');
    });
  }

  function loadSample(slug) {
    if (!window.fetch) { loadEmbeddedFallback('无 fetch'); return; }
    var url = 'data/' + encodeURIComponent(slug) + '.json';
    fetch(url, { cache: 'no-store' }).then(function (res) {
      if (!res.ok) throw new Error('HTTP ' + res.status);
      return res.json();
    }).then(function (json) {
      loadData(json, url);
      showToast('已载入样本 ' + slug);
    }).catch(function (err) {
      showToast('样本载入失败：' + (err && err.message ? err.message : err));
    });
  }

  function bindFileInput() {
    el('#file-input').addEventListener('change', function (ev) {
      var file = ev.target.files && ev.target.files[0];
      if (!file) return;
      var reader = new FileReader();
      reader.onload = function () {
        try {
          var json = JSON.parse(String(reader.result));
          loadData(json, file.name);
          showToast('已载入 ' + file.name);
        } catch (e) {
          showToast('JSON 解析失败：' + (e && e.message ? e.message : e));
        }
      };
      reader.onerror = function () { showToast('文件读取失败'); };
      reader.readAsText(file, 'utf-8');
      ev.target.value = '';
    });
  }

  /* ------------------------------------------------------------------ *
   * 初始化
   * ------------------------------------------------------------------ */
  function init() {
    el('#btn-first').addEventListener('click', function () { setPlaying(false); goToStep(0); });
    el('#btn-prev').addEventListener('click', function () { setPlaying(false); goToStep(state.current - 1); });
    el('#btn-next').addEventListener('click', function () { setPlaying(false); goToStep(state.current + 1); });
    el('#btn-last').addEventListener('click', function () {
      setPlaying(false);
      goToStep((state.data.steps || []).length - 1);
    });
    el('#btn-play').addEventListener('click', function () { setPlaying(!state.playing); });
    el('#speed').addEventListener('change', function (e) {
      state.speed = parseFloat(e.target.value) || 1;
    });

    el('#tab-table').addEventListener('click', function () { switchView('table'); });
    el('#tab-column').addEventListener('click', function () { switchView('column'); });
    el('#tab-mermaid').addEventListener('click', function () { switchView('mermaid'); });

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

    var sampleSel = el('#sample');
    if (sampleSel) {
      sampleSel.addEventListener('change', function (e) {
        setPlaying(false);
        loadSample(e.target.value);
      });
    }
    bindFileInput();
    updateControls();
    loadDefault();
  }

  function switchView(view) {
    state.view = view;
    el('#tab-table').classList.toggle('active', view === 'table');
    el('#tab-column').classList.toggle('active', view === 'column');
    el('#tab-mermaid').classList.toggle('active', view === 'mermaid');
    renderRight();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
