(function () {
  'use strict';

  var P = window.PAIGU;
  var M = window.PAIGU_MANAGE;
  function $(id) { return document.getElementById(id); }
  function esc(s) { return P.esc(s); }

  var PHASE_OPTIONS = ['Phase0', 'PhaseI', 'PhaseII', 'PhaseIII', 'Settling', 'Locked'];

  var KINDS = [
    { v: 'group', label: '拼团', variants: true },
    { v: 'single', label: '单领', variants: false },
    { v: 'box', label: '整盒', variants: false },
    { v: 'gift', label: '特典', variants: true }
  ];

  function kindMeta(v) {
    for (var i = 0; i < KINDS.length; i++) { if (KINDS[i].v === v) return KINDS[i]; }
    return KINDS[0];
  }
  function isRowKind(v) { return v === 'single' || v === 'box'; }

  var state = {
    config: null,
    revision: null,
    activeRoundId: null,
    items: [],
    phases: [],
    rounds: [],
    activeKind: 'group',
    conflicts: {},
    aliasTimer: null
  };

  function apiBase() {
    try {
      var s = window.localStorage.getItem('paigu.display.api');
      if (s) return P.stripSlash(s);
    } catch (e) { /* ignore */ }
    return P.resolveApiBase();
  }

  function banner(msg, bad) {
    var b = $('banner');
    b.textContent = msg;
    b.className = 'banner show' + (bad ? ' bad' : '');
  }
  function hideBanner() { $('banner').className = 'banner'; }
  function setConn(kind, text) {
    var el = $('conn-badge');
    el.className = 'status-badge ' + (kind || '');
    el.innerHTML = '<span class="dot"></span>' + esc(text);
  }
  function setRev() {
    $('rev-badge').textContent = 'revision ' + (state.revision == null ? '—' : state.revision);
  }
  function val(id) { var el = $(id); return el ? el.value : ''; }
  function setVal(id, v) { var el = $(id); if (el) el.value = v == null ? '' : v; }

  function intOr0(v) { var n = parseInt(v, 10); return isNaN(n) ? 0 : n; }
  function adjStr(v) { return v > 0 ? '+' + v : String(v); }

  function relDay(ms) {
    var shell = window.PAIGU_SHELL;
    return shell && shell.relDay ? shell.relDay(ms) : '';
  }

  /* ------------------------------ 数据模型 ------------------------------ */

  function normKind(kind, variants) {
    var s = String(kind == null ? '' : kind).trim().toLowerCase();
    if (s === '拼团' || s === 'group' || s === 'split') return 'group';
    if (s === '单领' || s === 'single') return 'single';
    if (s === '整盒' || s === 'box' || s === 'fullbox') return 'box';
    if (s === '特典' || s === 'gift') return 'gift';
    return (variants && variants.length) ? 'group' : 'single';
  }

  function normVariant(v) {
    v = v || {};
    var aliases = M.asStrArray(v.aliases);
    return {
      variant_id: v.variant_id == null ? '' : String(v.variant_id),
      name: v.name == null ? '' : String(v.name),
      unit_price_cents: P.num(v.unit_price_cents, 0),
      adjust_cents: P.num(v.adjust_cents != null ? v.adjust_cents : v.price_adjust_cents, 0),
      capacity: (v.capacity == null || v.capacity === '') ? null : P.num(v.capacity, null),
      aliases: aliases,
      __locked: aliases.length > 0
    };
  }

  function normItem(it) {
    it = it || {};
    var variants = (Array.isArray(it.variants) ? it.variants : []).map(normVariant);
    var aliases = M.asStrArray(it.aliases);
    return {
      item_id: it.item_id == null ? '' : String(it.item_id),
      name: it.name == null ? '' : String(it.name),
      kind: normKind(it.kind, variants),
      aliases: aliases,
      unit_price_cents: P.num(it.unit_price_cents, 0),
      max_quantity: (it.max_quantity == null || it.max_quantity === '') ? null : P.num(it.max_quantity, null),
      variants: variants,
      __locked: aliases.length > 0
    };
  }

  function serializeItem(it) {
    var meta = kindMeta(it.kind);
    var hasVar = meta.variants;
    var out = {
      item_id: it.item_id,
      name: it.name,
      kind: it.kind,
      class: hasVar ? 'A' : 'B',
      aliases: M.dedupe(it.aliases),
      unit_price_cents: intOr0(it.unit_price_cents),
      max_quantity: (it.kind === 'single' && it.max_quantity != null) ? intOr0(it.max_quantity) : null
    };
    if (hasVar) {
      out.variants = it.variants.map(function (v) {
        return {
          variant_id: v.variant_id,
          name: v.name,
          unit_price_cents: intOr0(v.unit_price_cents),
          adjust_cents: intOr0(v.adjust_cents),
          capacity: v.capacity == null ? null : intOr0(v.capacity),
          aliases: M.dedupe(v.aliases)
        };
      });
    } else {
      out.variants = [];
    }
    return out;
  }

  function nextItemId() {
    var max = 0;
    state.items.forEach(function (it) {
      var m = /^item_(\d+)$/.exec(it.item_id);
      if (m) max = Math.max(max, parseInt(m[1], 10));
    });
    return 'item_' + (max + 1);
  }

  function nextVariantId() {
    var max = 0;
    state.items.forEach(function (it) {
      (it.variants || []).forEach(function (v) {
        var m = /^v_(\d+)$/.exec(v.variant_id);
        if (m) max = Math.max(max, parseInt(m[1], 10));
      });
    });
    return 'v_' + (max + 1);
  }

  /* ------------------------------ 别名冲突 ------------------------------ */

  function computeConflicts() {
    var map = {};
    function add(token, idx) {
      var t = String(token == null ? '' : token).trim();
      if (!t) return;
      var e = map[t] || (map[t] = {});
      e[idx] = true;
    }
    state.items.forEach(function (it, i) {
      it.aliases.forEach(function (a) { add(a, i); });
      if (kindMeta(it.kind).variants) {
        it.variants.forEach(function (v) {
          add(v.name, i);
          v.aliases.forEach(function (a) { add(a, i); });
        });
      }
    });
    state.conflicts = map;
  }

  function cfTokens(host) {
    var idx = parseInt(host.getAttribute('data-idx'), 10);
    var it = state.items[idx];
    if (!it) return [];
    var scope = host.getAttribute('data-scope');
    var f = host.getAttribute('data-f');
    if (scope === 'item') {
      if (f === 'name') return it.name ? [it.name] : [];
      return it.aliases;
    }
    if (scope === 'variant') {
      var vi = parseInt(host.getAttribute('data-vi'), 10);
      var v = it.variants[vi];
      if (!v) return [];
      if (f === 'name') return v.name ? [v.name] : [];
      return v.aliases;
    }
    return [];
  }

  function paintConflicts() {
    computeConflicts();
    var hosts = document.querySelectorAll('#items [data-cf]');
    var anyConflict = false;
    for (var i = 0; i < hosts.length; i++) {
      var host = hosts[i];
      var idx = parseInt(host.getAttribute('data-idx'), 10);
      var toks = cfTokens(host);
      var bad = [];
      for (var t = 0; t < toks.length; t++) {
        var owners = state.conflicts[toks[t]];
        if (!owners) continue;
        for (var k in owners) {
          if (Object.prototype.hasOwnProperty.call(owners, k) && +k !== idx) { bad.push(toks[t]); break; }
        }
      }
      var span = host.querySelector('.conflict');
      if (!span) continue;
      if (bad.length) {
        anyConflict = true;
        span.classList.remove('hidden');
        span.textContent = '冲突：' + M.dedupe(bad).join('、') + '（与其它商品重复）';
      } else {
        span.classList.add('hidden');
        span.textContent = '';
      }
    }
    return anyConflict;
  }

  function scheduleConflictCheck(serverFallback) {
    if (state.aliasTimer) window.clearTimeout(state.aliasTimer);
    state.aliasTimer = window.setTimeout(function () {
      var any = paintConflicts();
      if (serverFallback && any) checkRound(state.activeRoundId, true);
    }, 120);
  }

  /* ------------------------------ 别名块 ------------------------------ */

  function aliasWrapHtml(scope, idx, vi, aliases, locked) {
    var viAttr = vi == null ? '' : ' data-vi="' + vi + '"';
    return '<div class="alias-wrap cf-host" data-cf="1" data-scope="' + scope + '" data-f="aliases" data-idx="' + idx + '"' + viAttr + '>' +
      '<input class="alias-input" data-scope="' + scope + '" data-f="aliases" data-idx="' + idx + '"' + viAttr +
      ' value="' + esc((aliases || []).join(', ')) + '" placeholder="别名，逗号/顿号/斜杠分隔" />' +
      '<span class="alias-tools">' +
      '<button type="button" class="lock-btn' + (locked ? ' locked' : '') + '" data-act="lock" data-scope="' + scope + '" data-idx="' + idx + '"' + viAttr +
      ' title="' + (locked ? '已锁定：不参与 LLM 建议、不从商品名更新（点击解锁）' : '未锁定：可从商品名自动更新（点击锁定）') + '">' +
      (locked ? '🔒' : '🔓') + '</button>' +
      '<button type="button" class="small" data-act="split" data-scope="' + scope + '" data-idx="' + idx + '"' + viAttr +
      ' title="本地词组切分：按空白/逗号/顿号/斜杠切分、去重、trim">切分</button>' +
      '</span>' +
      '<span class="conflict hidden"></span>' +
      '</div>';
  }

  function cfNameHtml(scope, idx, vi, f, value, cls, extra) {
    var viAttr = vi == null ? '' : ' data-vi="' + vi + '"';
    return '<div class="cf-host" data-cf="1" data-scope="' + scope + '" data-f="' + f + '" data-idx="' + idx + '"' + viAttr + '>' +
      '<input class="' + (cls || '') + '" data-scope="' + scope + '" data-f="' + f + '" data-idx="' + idx + '"' + viAttr +
      ' value="' + esc(value) + '"' + (extra || '') + ' />' +
      '<span class="conflict hidden"></span>' +
      '</div>';
  }

  /* ------------------------------ 渲染：种类 tab ------------------------------ */

  function renderKindTabs() {
    var host = $('kind-tabs');
    if (!host) return;
    var html = '';
    KINDS.forEach(function (k) {
      var cnt = state.items.filter(function (it) { return it.kind === k.v; }).length;
      html += '<button type="button" data-kind="' + k.v + '"' + (state.activeKind === k.v ? ' class="active"' : '') + '>' +
        esc(k.label) + '<span class="cnt">' + cnt + '</span></button>';
    });
    host.innerHTML = html;
  }

  /* ------------------------------ 渲染：商品 ------------------------------ */

  function makeGap(gidx) {
    var g = document.createElement('div');
    g.className = 'gap';
    g.setAttribute('data-gap', String(gidx));
    g.innerHTML = '<button type="button" class="gap-plus" title="在此处插入商品" aria-label="在此处插入商品">＋</button>';
    g.addEventListener('mouseenter', function () { g.classList.add('open'); });
    g.addEventListener('mouseleave', function () { if (!g.__temp) g.classList.remove('open'); });
    g.querySelector('.gap-plus').addEventListener('click', function (e) {
      e.stopPropagation();
      openTempAt(g, gidx, true);
    });
    return g;
  }

  function renderItems() {
    var host = $('items');
    if (!host) return;
    while (host.firstChild) host.removeChild(host.firstChild);
    var list = [];
    state.items.forEach(function (it, i) { if (it.kind === state.activeKind) list.push({ it: it, i: i }); });

    if (isRowKind(state.activeKind)) renderRows(host, list, state.activeKind === 'single');
    else renderCards(host, list);

    if (!list.length) {
      var e = document.createElement('div');
      e.className = 'empty';
      e.textContent = '当前种类暂无商品，点击下方虚线卡片添加';
      host.appendChild(e);
    }
    scheduleConflictCheck(false);
  }

  function renderRows(host, list, isSingle) {
    var head = document.createElement('div');
    head.className = 'single-row single-head';
    head.innerHTML = '<span></span><span>商品名</span><span>原价（分）</span>' +
      '<span>' + (isSingle ? '上限' : '数量') + '</span><span>别名（锁定后不从商品名更新）</span><span></span>';
    host.appendChild(head);

    list.forEach(function (entry, k) {
      host.appendChild(makeGap(entry.i));
      host.appendChild(makeSingleRow(entry.it, entry.i, isSingle, k + 1));
    });
  }

  function makeSingleRow(it, idx, isSingle, rownum) {
    var row = document.createElement('div');
    row.className = 'single-row';
    row.setAttribute('data-idx', String(idx));
    var maxq = isSingle
      ? '<input data-scope="item" data-f="max_quantity" data-idx="' + idx + '" type="number" min="0" value="' +
        (it.max_quantity == null ? '' : esc(it.max_quantity)) + '" placeholder="上限" />'
      : '<span class="hint" title="整盒无单领上限">—</span>';
    row.innerHTML =
      '<span class="rownum">#' + rownum + '</span>' +
      '<input data-scope="item" data-f="name" data-idx="' + idx + '" value="' + esc(it.name) + '" placeholder="商品名（填完自动产生下一行）" title="item_id: ' + esc(it.item_id) + '" />' +
      '<input data-scope="item" data-f="unit_price_cents" data-idx="' + idx + '" type="number" step="1" value="' + esc(it.unit_price_cents) + '" placeholder="原价(分)" />' +
      '<span class="maxq-cell">' + maxq + '</span>' +
      aliasWrapHtml('item', idx, null, it.aliases, it.__locked) +
      '<span class="row-actions"><button type="button" class="danger small" data-act="del-item" data-idx="' + idx + '">删除</button></span>';
    return row;
  }

  function renderCards(host, list) {
    list.forEach(function (entry) {
      host.appendChild(makeGap(entry.i));
      host.appendChild(makeItemCard(entry.it, entry.i));
    });
  }

  function makeItemCard(it, idx) {
    var meta = kindMeta(it.kind);
    var derived = it.variants.length ? 'A' : 'B';
    var card = document.createElement('div');
    card.className = 'cat-item';
    card.setAttribute('data-idx', String(idx));

    var kindOpts = KINDS.map(function (k) {
      return '<option value="' + k.v + '"' + (it.kind === k.v ? ' selected' : '') + '>' + esc(k.label) + '</option>';
    }).join('');

    var head = document.createElement('div');
    head.className = 'cat-item-head';
    head.innerHTML = '<span class="idx">商品 #' + (idx + 1) + '</span>' +
      '<label class="row tight"><span class="hint">种类</span><select data-scope="item" data-f="kind" data-idx="' + idx + '">' + kindOpts + '</select></label>' +
      '<span class="derived">推导 class：<b>' + derived + '</b>' + (derived === 'A' ? '（阶段受限）' : '（不限）') + '</span>' +
      '<span class="spacer" style="flex:1"></span>' +
      (meta.variants ? '<button type="button" class="small" data-act="add-variant" data-idx="' + idx + '">添加变体</button>' : '') +
      '<button type="button" class="danger small" data-act="del-item" data-idx="' + idx + '">删除商品</button>';
    card.appendChild(head);

    var grid = document.createElement('div');
    grid.className = 'cat-grid';
    grid.innerHTML =
      '<label class="field"><span>item_id</span><input data-scope="item" data-f="item_id" data-idx="' + idx + '" value="' + esc(it.item_id) + '" /></label>' +
      '<label class="field"><span>商品名 name</span><input data-scope="item" data-f="name" data-idx="' + idx + '" value="' + esc(it.name) + '" placeholder="商品名" /></label>' +
      '<label class="field"><span>原价 unit_price_cents（分）</span><input data-scope="item" data-f="unit_price_cents" data-idx="' + idx + '" type="number" step="1" value="' + esc(it.unit_price_cents) + '" /></label>' +
      (it.kind === 'single'
        ? '<label class="field"><span>单领上限 max_quantity</span><input data-scope="item" data-f="max_quantity" data-idx="' + idx + '" type="number" min="0" value="' + (it.max_quantity == null ? '' : esc(it.max_quantity)) + '" /></label>'
        : '');
    card.appendChild(grid);

    var aliasRow = document.createElement('div');
    aliasRow.style.marginTop = '8px';
    aliasRow.innerHTML = '<div class="hint" style="margin-bottom:3px">商品别名（LLM 建议针对此层；锁定后不参与）</div>' +
      aliasWrapHtml('item', idx, null, it.aliases, it.__locked);
    card.appendChild(aliasRow);

    if (meta.variants) {
      var wrap = document.createElement('div');
      wrap.className = 'vtable-scroll';
      var rows = it.variants.map(function (v, vi) {
        var fin = intOr0(v.unit_price_cents) + intOr0(v.adjust_cents);
        var clsB = v.adjust_cents > 0 ? ' price-pos' : (v.adjust_cents < 0 ? ' price-neg' : '');
        var clsC = fin > 0 ? ' price-pos' : (fin < 0 ? ' price-neg' : '');
        return '<tr data-vi="' + vi + '">' +
          '<td class="vid" title="自动生成，不可编辑">' + esc(v.variant_id || '(空)') + '</td>' +
          '<td class="vname">' + cfNameHtml('variant', idx, vi, 'name', v.name, '', ' placeholder="变体名"') + '</td>' +
          '<td class="pcell"><input class="variant-price-base" data-scope="variant" data-f="unit_price_cents" data-idx="' + idx + '" data-vi="' + vi + '" type="number" step="1" value="' + esc(v.unit_price_cents) + '" title="原价 A" /></td>' +
          '<td class="pcell"><input class="variant-price-adjust' + clsB + '" data-scope="variant" data-f="adjust_cents" data-idx="' + idx + '" data-vi="' + vi + '" type="text" inputmode="numeric" value="' + esc(adjStr(v.adjust_cents)) + '" title="调价 B" /></td>' +
          '<td class="pcell"><input class="variant-price-final' + clsC + '" data-scope="variant" data-f="final" data-idx="' + idx + '" data-vi="' + vi + '" type="text" inputmode="numeric" value="' + esc(fin) + '" title="最终价 C = A + B" /></td>' +
          '<td class="pcell"><input data-scope="variant" data-f="capacity" data-idx="' + idx + '" data-vi="' + vi + '" type="number" min="0" value="' + (v.capacity == null ? '' : esc(v.capacity)) + '" placeholder="上限" /></td>' +
          '<td>' + aliasWrapHtml('variant', idx, vi, v.aliases, v.__locked) + '</td>' +
          '<td><button type="button" class="danger small" data-act="del-variant" data-idx="' + idx + '" data-vi="' + vi + '">×</button></td>' +
          '</tr>';
      }).join('');
      wrap.innerHTML = '<table class="vtable"><thead><tr>' +
        '<th>变体 ID（只读·自动）</th><th>名称</th><th>原价 A</th><th>调价 B</th><th>最终价 C</th><th>上限</th><th>别名</th><th>操作</th>' +
        '</tr></thead><tbody>' + rows + '</tbody></table>' +
        '<div class="hint">A/B/C 联动：改 A 或 B ⇒ C 变；改 C ⇒ B 变；A 永不联动。<span class="price-pos">+ 红</span> / <span class="price-neg">- 蓝</span>。数量限制按整盒/整体处理，故已移除 box_size 与 pieces。</div>';
      card.appendChild(wrap);
    } else {
      var hint = document.createElement('div');
      hint.className = 'hint';
      hint.style.marginTop = '6px';
      hint.textContent = '该种类无变体（' + (it.kind === 'box' ? '整盒：商品级原价，数量按盒' : '单领：商品级原价') + '）。';
      card.appendChild(hint);
    }
    return card;
  }

  /* ------------------------------ 临时添加卡片 ------------------------------ */

  function openTempAt(anchor, gidx, autoRemove) {
    if (!anchor) return;
    if (anchor.__temp && anchor.__temp.isConnected) return;
    anchor.__temp = null;
    var kindOpts = KINDS.map(function (k) {
      return '<option value="' + k.v + '"' + (state.activeKind === k.v ? ' selected' : '') + '>' + esc(k.label) + '</option>';
    }).join('');
    var temp = document.createElement('div');
    temp.className = 'dashed temp';
    temp.innerHTML = '<div class="hint" style="margin-bottom:6px">选择「种类」后添加商品' + (autoRemove ? '（未确认则鼠标移开自动消失）' : '') + '</div>' +
      '<div class="add-row"><label class="row tight"><span class="hint">种类</span>' +
      '<select data-temp-kind>' + kindOpts + '</select></label></div>' +
      '<div class="temp-actions">' +
      '<button type="button" data-act="temp-cancel" class="small">取消</button>' +
      '<button type="button" data-act="temp-confirm" class="primary small">添加</button>' +
      '</div>';
    anchor.parentNode.insertBefore(temp, anchor.nextSibling);
    anchor.__temp = temp;
    anchor.classList.add('open');

    function close() {
      if (temp.parentNode) temp.parentNode.removeChild(temp);
      anchor.__temp = null;
      anchor.classList.remove('open');
    }
    temp.__close = close;

    temp.addEventListener('click', function (e) {
      var b = e.target.closest ? e.target.closest('[data-act]') : null;
      if (!b) return;
      var act = b.getAttribute('data-act');
      if (act === 'temp-confirm') {
        var sel = temp.querySelector('[data-temp-kind]');
        var kind = sel ? sel.value : state.activeKind;
        if (state.items.length >= 0) { /* noop */ }
        state.items.splice(Math.min(gidx, state.items.length), 0, newItem(kind));
        close();
        state.activeKind = kind;
        renderKindTabs();
        renderItems();
      } else if (act === 'temp-cancel') {
        close();
      }
    });

    if (autoRemove) {
      var t = null;
      temp.addEventListener('mouseleave', function () {
        t = window.setTimeout(close, 180);
      });
      temp.addEventListener('mouseenter', function () {
        if (t) { window.clearTimeout(t); t = null; }
      });
    }
    return temp;
  }

  function newItem(kind) {
    return {
      item_id: nextItemId(),
      name: '',
      kind: kind,
      aliases: [],
      unit_price_cents: 0,
      max_quantity: null,
      variants: kindMeta(kind).variants ? [newVariant()] : [],
      __locked: false
    };
  }

  function newVariant() {
    return {
      variant_id: nextVariantId(),
      name: '',
      unit_price_cents: 0,
      adjust_cents: 0,
      capacity: null,
      aliases: [],
      __locked: false
    };
  }

  /* ------------------------------ 输入事件 ------------------------------ */

  function findAliasInput(scope, idx, vi) {
    var sel = '#items .alias-input[data-scope="' + scope + '"][data-idx="' + idx + '"]';
    if (vi != null) sel += '[data-vi="' + vi + '"]';
    return document.querySelector(sel);
  }

  function updateLockBtn(scope, idx, vi, locked) {
    var sel = '#items .lock-btn[data-scope="' + scope + '"][data-idx="' + idx + '"]';
    if (vi != null) sel += '[data-vi="' + vi + '"]';
    var btn = document.querySelector(sel);
    if (!btn) return;
    btn.classList.toggle('locked', locked);
    btn.textContent = locked ? '🔒' : '🔓';
    btn.title = locked
      ? '已锁定：不参与 LLM 建议、不从商品名更新（点击解锁）'
      : '未锁定：可从商品名自动更新（点击锁定）';
  }

  function autoAlias(scope, idx, vi, name) {
    var inp = findAliasInput(scope, idx, vi);
    var owner;
    var it = state.items[idx];
    if (!it) return;
    owner = scope === 'item' ? it : it.variants[vi];
    if (!owner || owner.__locked) return;
    owner.aliases = name && name.trim() ? [name.trim()] : [];
    if (inp) inp.value = owner.aliases.join(', ');
  }

  function onInput(ev) {
    var t = ev.target;
    if (!t || !t.dataset || !t.dataset.scope) return;
    var idx = parseInt(t.dataset.idx, 10);
    var it = state.items[idx];
    if (!it) return;
    var f = t.dataset.f;

    if (t.dataset.scope === 'item') {
      if (f === 'name') {
        it.name = t.value;
        autoAlias('item', idx, null, t.value);
        maybeGrowSingles(it, t);
        scheduleConflictCheck(true);
      } else if (f === 'item_id') {
        it.item_id = t.value;
      } else if (f === 'unit_price_cents') {
        it.unit_price_cents = intOr0(t.value);
      } else if (f === 'max_quantity') {
        it.max_quantity = t.value.trim() === '' ? null : intOr0(t.value);
      } else if (f === 'aliases') {
        it.aliases = M.splitTokens(t.value);
        it.__locked = true;
        updateLockBtn('item', idx, null, true);
        scheduleConflictCheck(true);
      }
    } else if (t.dataset.scope === 'variant') {
      var vi = parseInt(t.dataset.vi, 10);
      var v = it.variants[vi];
      if (!v) return;
      if (f === 'name') {
        v.name = t.value;
        autoAlias('variant', idx, vi, t.value);
        scheduleConflictCheck(true);
      } else if (f === 'unit_price_cents') {
        v.unit_price_cents = intOr0(t.value);
        syncFinal(t);
      } else if (f === 'adjust_cents') {
        v.adjust_cents = intOr0(t.value);
        paintPrice(t);
        syncFinal(t);
      } else if (f === 'final') {
        if (String(t.value).trim() === '' || String(t.value).trim() === '-' || String(t.value).trim() === '+') {
          paintPrice(t);
          return;
        }
        var c = intOr0(t.value);
        v.adjust_cents = c - intOr0(v.unit_price_cents);
        var adj = t.closest('tr').querySelector('.variant-price-adjust');
        if (adj) { adj.value = adjStr(v.adjust_cents); paintPrice(adj); }
        paintPrice(t);
      } else if (f === 'capacity') {
        v.capacity = t.value.trim() === '' ? null : intOr0(t.value);
      } else if (f === 'aliases') {
        v.aliases = M.splitTokens(t.value);
        v.__locked = true;
        updateLockBtn('variant', idx, vi, true);
        scheduleConflictCheck(true);
      }
    }
  }

  function onChange(ev) {
    var t = ev.target;
    if (!t || !t.dataset || !t.dataset.scope) return;
    var idx = parseInt(t.dataset.idx, 10);
    var it = state.items[idx];
    if (!it) return;
    var f = t.dataset.f;
    if (t.dataset.scope === 'item' && f === 'kind') {
      it.kind = normKind(t.value, it.variants);
      state.activeKind = it.kind;
      renderKindTabs();
      renderItems();
      return;
    }
    if (t.dataset.scope === 'item' && f === 'aliases') {
      t.value = it.aliases.join(', ');
    } else if (t.dataset.scope === 'variant' && f === 'aliases') {
      var vi = parseInt(t.dataset.vi, 10);
      if (it.variants[vi]) t.value = it.variants[vi].aliases.join(', ');
    }
    scheduleConflictCheck(true);
  }

  function onClick(ev) {
    var t = ev.target;
    var b = t && t.closest ? t.closest('[data-act]') : null;
    if (!b) return;
    var act = b.getAttribute('data-act');
    var idx = parseInt(b.getAttribute('data-idx'), 10);
    var vi = b.hasAttribute('data-vi') ? parseInt(b.getAttribute('data-vi'), 10) : null;
    var it = state.items[idx];

    if (act === 'del-item') {
      if (!it) return;
      state.items.splice(idx, 1);
      renderKindTabs();
      renderItems();
    } else if (act === 'add-variant') {
      if (!it) return;
      it.variants.push(newVariant());
      renderItems();
    } else if (act === 'del-variant') {
      if (!it || !it.variants[vi]) return;
      it.variants.splice(vi, 1);
      renderItems();
    } else if (act === 'lock') {
      var scope = b.getAttribute('data-scope');
      var owner = scope === 'item' ? it : (it && it.variants[vi]);
      if (!owner) return;
      owner.__locked = !owner.__locked;
      updateLockBtn(scope, idx, vi, owner.__locked);
      if (!owner.__locked) scheduleConflictCheck(true);
    } else if (act === 'split') {
      var scope2 = b.getAttribute('data-scope');
      var owner2 = scope2 === 'item' ? it : (it && it.variants[vi]);
      var inp = findAliasInput(scope2, idx, vi);
      if (!owner2 || !inp) return;
      owner2.aliases = M.dedupe(M.splitTokens(inp.value));
      owner2.__locked = true;
      inp.value = owner2.aliases.join(', ');
      updateLockBtn(scope2, idx, vi, true);
      scheduleConflictCheck(true);
    }
  }

  function syncFinal(t) {
    var tr = t.closest('tr');
    if (!tr) return;
    var base = tr.querySelector('.variant-price-base');
    var adj = tr.querySelector('.variant-price-adjust');
    var fin = tr.querySelector('.variant-price-final');
    if (!fin) return;
    var c = intOr0(base && base.value) + intOr0(adj && adj.value);
    fin.value = String(c);
    paintPrice(fin);
  }

  function paintPrice(inp) {
    if (!inp) return;
    var v = parseInt(inp.value, 10);
    if (isNaN(v)) v = 0;
    inp.classList.toggle('price-pos', v > 0);
    inp.classList.toggle('price-neg', v < 0);
  }

  /* 单领：填完商品名立刻产生新行；Tab 竖向优先 */
  function maybeGrowSingles(it, t) {
    if (!it || state.activeKind !== 'single') return;
    if (!it.name || !it.name.trim()) return;
    var row = t.closest('.single-row');
    var rows = document.querySelectorAll('#items .single-row:not(.single-head)');
    if (!row || rows.length === 0 || rows[rows.length - 1] !== row) return;
    state.items.push(newItem('single'));
    var host = $('items');
    host.appendChild(makeGap(state.items.length - 1));
    host.appendChild(makeSingleRow(state.items[state.items.length - 1], state.items.length - 1, true, rows.length + 1));
    renderKindTabs();
  }

  function onKeydown(ev) {
    if (ev.key !== 'Tab') return;
    var t = ev.target;
    if (!t || !t.closest || !t.closest('.single-row')) return;
    if (state.activeKind !== 'single') return;
    ev.preventDefault();
    var row = t.closest('.single-row');
    var inputs = row.querySelectorAll('input');
    var list = Array.prototype.slice.call(inputs);
    var col = list.indexOf(t);
    if (col < 0) col = 0;
    var rows = Array.prototype.slice.call(document.querySelectorAll('#items .single-row:not(.single-head)'));
    var ri = rows.indexOf(row);
    var dir = ev.shiftKey ? -1 : 1;
    var target = rows[ri + dir];
    if (!target) {
      if (!ev.shiftKey) {
        var last = state.items[state.items.length - 1];
        if (last && last.kind === 'single' && last.name && last.name.trim()) {
          state.items.push(newItem('single'));
          var host = $('items');
          host.appendChild(makeGap(state.items.length - 1));
          host.appendChild(makeSingleRow(state.items[state.items.length - 1], state.items.length - 1, true, rows.length + 1));
          renderKindTabs();
          target = document.querySelectorAll('#items .single-row:not(.single-head)')[rows.length];
        }
      }
      if (!target) return;
    }
    var tin = target.querySelectorAll('input');
    var want = Math.min(col, tin.length - 1);
    if (tin[want]) { tin[want].focus(); if (tin[want].select) tin[want].select(); }
  }

  function onFocusOut(ev) {
    var t = ev.target;
    if (t && t.classList && t.classList.contains('alias-input')) {
      onChange({ target: t });
    }
    scheduleConflictCheck(true);
  }

  /* ------------------------------ LLM 建议 ------------------------------ */

  function setLlmStatus(text, cls) {
    var el = $('alias-llm-status');
    if (!el) return;
    el.textContent = text || '';
    el.className = 'status-inline' + (cls ? ' ' + cls : '');
  }

  function suggestAliases() {
    if (state.llmBusy) return;
    var entries = [];
    state.items.forEach(function (it) {
      if (it.__locked) return;
      entries.push({ item_id: it.item_id, name: it.name, aliases: M.dedupe(it.aliases) });
    });
    if (!entries.length) { setLlmStatus('没有可请求的别名（均已锁定或为空）', 'bad'); return; }
    state.llmBusy = true;
    $('alias-llm-btn').disabled = true;
    setLlmStatus('等待回复…', 'busy');
    P.post('/api/items/suggest-aliases', { items: entries, mode: 'aliases' }).then(function (res) {
      var sugg = (res && res.suggestions) || [];
      var filled = 0;
      var best = 0;
      var byId = {};
      state.items.forEach(function (it, i) { byId[it.item_id] = i; });
      sugg.forEach(function (s) {
        if (!s || s.item_id == null) return;
        var i = byId[s.item_id];
        if (i == null) return;
        var it = state.items[i];
        if (!it || it.__locked) return;
        if (s.verdict === 'best') { best++; return; }
        if (Array.isArray(s.aliases)) {
          it.aliases = M.dedupe(s.aliases);
          var inp = findAliasInput('item', i, null);
          if (inp) inp.value = it.aliases.join(', ');
          filled++;
        }
      });
      scheduleConflictCheck(true);
      if (filled > 0) setLlmStatus('已填入新方案（' + filled + ' 项）', 'ok');
      else if (best > 0) setLlmStatus('是最佳（' + best + ' 项）', 'ok');
      else setLlmStatus('无建议', '');
    }).catch(function (err) {
      var notReady = err && (err.status === 404 || err.status === 405 || err.status === 501);
      var msg = notReady
        ? '接口 /api/items/suggest-aliases 未就绪（集成轮提供），已跳过'
        : 'LLM 建议失败：' + P.errorText(err);
      setLlmStatus(msg, 'bad');
    }).then(function () {
      state.llmBusy = false;
      $('alias-llm-btn').disabled = false;
    });
  }

  /* ------------------------------ 轮次管理 ------------------------------ */

  function setRoundsInfo(text) { var el = $('rounds-info'); if (el) el.textContent = text || ''; }

  function loadRounds() {
    setRoundsInfo('载入中…');
    return P.get('/api/rounds').then(function (res) {
      state.rounds = (res && res.rounds) || [];
      state.activeRoundId = res && res.active_round_id;
      renderRounds();
      renderCopyOptions();
      setRoundsInfo(state.rounds.length + ' 个轮次');
    }).catch(function (err) {
      setRoundsInfo('');
      banner('轮次列表加载失败：' + P.errorText(err), true);
    });
  }

  function renderRounds() {
    var host = $('rounds-list');
    if (!host) return;
    while (host.firstChild) host.removeChild(host.firstChild);
    var list = state.rounds || [];
    $('rounds-empty').hidden = list.length > 0;
    list.forEach(function (r) {
      host.appendChild(makeRoundCard(r));
    });
  }

  function makeRoundCard(r) {
    var active = !!r.active;
    var card = document.createElement('div');
    card.className = 'round-card' + (active ? ' active' : '');
    var updated = r.updated_at ? String(r.updated_at).replace('T', ' ').slice(0, 19) : '—';
    var actions =
      '<button type="button" class="small" data-act="activate" data-mode="continue" data-id="' + esc(r.round_id) + '">激活(continue)</button>' +
      '<button type="button" class="small" data-act="activate" data-mode="fresh" data-id="' + esc(r.round_id) + '">激活并清空(fresh)</button>' +
      '<button type="button" class="small" data-act="activate" data-mode="replay" data-id="' + esc(r.round_id) + '">激活并重放(replay)</button>' +
      '<button type="button" class="small" data-act="check" data-id="' + esc(r.round_id) + '">校验</button>' +
      '<button type="button" class="danger small" data-act="del-round" data-id="' + esc(r.round_id) + '"' + (active ? ' disabled title="激活中不可删除"' : '') + '>删除</button>';
    card.innerHTML =
      '<div class="rc-main">' +
      '<div class="rc-title">' + esc(r.title || r.round_id) + (active ? '<span class="badge-active">激活中</span>' : '') + '</div>' +
      '<div class="rc-id">round_id：' + esc(r.round_id) + '</div>' +
      '<div class="rc-meta">group_id ' + esc(r.group_id || '—') + ' · 商品 ' + esc(r.items == null ? '—' : r.items) +
      ' · 变体 ' + esc(r.variants == null ? '—' : r.variants) + ' · 更新 ' + esc(updated) + '</div>' +
      '</div>' +
      '<div class="rc-actions">' + actions + '</div>';

    card.addEventListener('click', function (e) {
      var b = e.target.closest ? e.target.closest('button[data-act]') : null;
      if (!b) return;
      b.disabled = true;
      var id = b.getAttribute('data-id');
      var act = b.getAttribute('data-act');
      if (act === 'activate') activateRound(id, b.getAttribute('data-mode'));
      else if (act === 'check') checkRound(id, false);
      else if (act === 'del-round') deleteRound(id);
    });
    return card;
  }

  function renderCopyOptions() {
    var sel = $('rd-new-copy');
    if (!sel) return;
    var cur = sel.value;
    var html = '<option value="">（不复制）</option>';
    (state.rounds || []).forEach(function (r) {
      html += '<option value="' + esc(r.round_id) + '">' + esc(r.title || r.round_id) + '（' + esc(r.round_id) + '）</option>';
    });
    sel.innerHTML = html;
    sel.value = cur;
  }

  function activateRound(id, mode) {
    if (!id) return;
    if (mode === 'fresh' && !window.confirm('激活并清空 ' + id + '：将重置内存排位，确认？')) { renderRounds(); return; }
    if (mode === 'replay' && !window.confirm('激活并重放 ' + id + '：将重置并自动计算重放，确认？')) { renderRounds(); return; }
    setConn('busy', '切换中…');
    P.post('/api/rounds/' + encodeURIComponent(id) + '/activate', { mode: mode }).then(function (res) {
      hideBanner();
      P.toast('已激活 ' + id + '（' + mode + '）', 'ok');
      setConn('ok', '已连接');
      if (mode === 'replay' && res && res.replayed) {
        banner('重放完成：version=' + (res.version == null ? '?' : res.version), false);
      }
      return load().then(loadRounds);
    }).catch(function (err) {
      setConn('bad', '切换失败');
      banner('激活失败：' + P.errorText(err), true);
      renderRounds();
    });
  }

  function checkRound(id, quiet) {
    if (!id) return;
    return P.post('/api/rounds/' + encodeURIComponent(id) + '/check', {}).then(function (res) {
      renderIssues(id, res);
      if (!quiet && res) P.toast(res.ok ? '校验通过' : '校验发现问题', res.ok ? 'ok' : 'bad');
    }).catch(function (err) {
      if (!quiet) banner('校验失败：' + P.errorText(err), true);
    });
  }

  function renderIssues(id, res) {
    var host = $('round-check');
    if (!host) return;
    while (host.firstChild) host.removeChild(host.firstChild);
    var issues = (res && res.issues) || [];
    var head = document.createElement('div');
    head.className = 'hint';
    head.style.marginTop = '8px';
    head.textContent = '校验 ' + id + '：' + (res && res.ok ? '通过' : '未通过') + '（' + issues.length + ' 项）';
    host.appendChild(head);
    if (!issues.length) return;
    var list = document.createElement('div');
    list.className = 'issue-list';
    issues.forEach(function (is) {
      var d = document.createElement('div');
      d.className = 'issue ' + (is.level === 'error' ? 'error' : 'warn');
      d.innerHTML = '<b>' + esc(is.level) + '</b><span>' + esc(is.message) + '</span><span class="where">@' + esc(is.where || '') + '</span>';
      list.appendChild(d);
    });
    host.appendChild(list);
  }

  function createRound() {
    var id = val('rd-new-id').trim();
    if (!id) { banner('请填写 round_id', true); return; }
    var body = { round_id: id, title: val('rd-new-title').trim() || undefined };
    var copy = val('rd-new-copy');
    if (copy) body.copy_from = copy;
    $('rd-new-btn').disabled = true;
    P.post('/api/rounds', body).then(function () {
      hideBanner();
      P.toast('轮次已创建：' + id, 'ok');
      setVal('rd-new-id', '');
      setVal('rd-new-title', '');
      setVal('rd-new-copy', '');
      return loadRounds();
    }).catch(function (err) {
      banner('创建轮次失败：' + P.errorText(err), true);
    }).then(function () {
      $('rd-new-btn').disabled = false;
    });
  }

  function deleteRound(id) {
    if (!window.confirm('确认删除轮次 ' + id + '？此操作不可撤销（激活中的轮次不可删除）。')) { renderRounds(); return; }
    P.request('/api/rounds/' + encodeURIComponent(id), { method: 'DELETE' }).then(function () {
      hideBanner();
      P.toast('轮次已删除：' + id, 'ok');
      return loadRounds();
    }).catch(function (err) {
      banner('删除轮次失败：' + P.errorText(err), true);
      renderRounds();
    });
  }

  /* ------------------------------ 配置 载入 / 保存 ------------------------------ */

  function updateWindowMs() {
    var s = M.localInputToMs(val('rd-start'));
    var e = M.localInputToMs(val('rd-end'));
    var el = $('rd-window-ms');
    if (!el) return;
    el.textContent = 'start_ms=' + (s == null ? '—' : s) + '（' + (s == null ? '—' : relDay(s)) + '） · ' +
      'end_ms=' + (e == null ? '—' : e) + '（' + (e == null ? '—' : relDay(e)) + '）';
  }

  function applyToForm(cfg) {
    var rd = cfg.round || {};
    setVal('rd-id', rd.round_id);
    setVal('rd-title', rd.title);
    setVal('rd-group', rd.group_id);
    setVal('rd-priority', M.joinList(rd.priority_users));
    var win = rd.priority_window || {};
    setVal('rd-start', M.msToLocalInput(win.start_ms));
    setVal('rd-end', M.msToLocalInput(win.end_ms));
    updateWindowMs();

    state.items = (Array.isArray(rd.items) ? rd.items : []).map(normItem);
    state.phases = P.deepClone(Array.isArray(rd.phases) ? rd.phases : []);
    var hasKind = {};
    state.items.forEach(function (it) { hasKind[it.kind] = true; });
    if (!hasKind[state.activeKind]) {
      state.activeKind = state.items.length ? state.items[0].kind : 'group';
    }
    renderKindTabs();
    renderItems();
    renderPhases();

    state.activeRoundId = cfg.active_round_id || rd.round_id;
    $('rd-active-info').textContent = rd.round_id ? ('round_id=' + rd.round_id) : '';
  }

  function collectConfig() {
    var cfg = P.deepClone(state.config) || {};
    cfg.round = cfg.round || {};
    cfg.round.round_id = val('rd-id').trim();
    cfg.round.title = val('rd-title');
    cfg.round.group_id = val('rd-group');
    cfg.round.priority_users = M.splitList(val('rd-priority'));
    var s = M.localInputToMs(val('rd-start'));
    var e = M.localInputToMs(val('rd-end'));
    if (s == null && e == null) {
      cfg.round.priority_window = null;
    } else {
      cfg.round.priority_window = { start_ms: s == null ? 0 : s, end_ms: e == null ? 0 : e };
    }
    cfg.round.phases = state.phases.map(function (p) {
      var ph = PHASE_OPTIONS.indexOf(p.phase) >= 0 ? p.phase : 'Phase0';
      return { phase: ph, start_ms: intOr0(p.start_ms), end_ms: intOr0(p.end_ms) };
    });
    cfg.round.items = state.items.map(serializeItem);
    return cfg;
  }

  function load() {
    setConn('', '载入中…');
    return P.get('/api/config').then(function (res) {
      var cfg = res && res.config ? res.config : res;
      state.config = cfg;
      state.revision = res && res.revision != null ? res.revision : (cfg ? cfg.revision : null);
      applyToForm(cfg || {});
      hideBanner();
      setRev();
      setConn('ok', '已连接');
      $('form').classList.remove('hidden');
      $('loading').classList.add('hidden');
    }).catch(function (err) {
      banner('无法载入配置：' + P.errorText(err) + '。请确认本地服务已在 ' + apiBase() + ' 运行。', true);
      setConn('bad', '未连接');
      $('loading').textContent = '配置载入失败（API 未就绪）';
      throw err;
    });
  }

  function save() {
    if (!state.config) { banner('尚未载入配置，无法保存。', true); return; }
    var hasConflict = paintConflicts();
    var cfg = collectConfig();
    $('save').disabled = true;
    P.put('/api/config', { config: cfg, revision: state.revision }).then(function (res) {
      state.config = res && res.config ? res.config : cfg;
      state.revision = res && res.revision != null ? res.revision : state.revision;
      applyToForm(state.config);
      if (hasConflict) {
        banner('已保存，但存在跨商品别名/变体名冲突（行内提醒），可用「校验」查看后端结果。', false);
      } else {
        hideBanner();
      }
      setRev();
      P.toast('配置已保存，revision ' + state.revision, 'ok');
    }).catch(function (err) {
      if (err.status === 409) {
        banner('配置冲突（HTTP 409）：服务器 revision 已变化，你的改动未保存。请点「重新载入」获取最新配置后重编辑。', true);
      } else {
        banner('保存失败：' + P.errorText(err), true);
      }
    }).then(function () {
      $('save').disabled = false;
    });
  }

  function reloadDisk() {
    $('reload').disabled = true;
    P.post('/api/config/reload', {}).then(function () { return load(); })
      .then(function () { P.toast('已从磁盘重载配置', 'ok'); })
      .catch(function (err) { banner('从磁盘重载失败：' + P.errorText(err), true); })
      .then(function () { $('reload').disabled = false; });
  }

  /* ------------------------------ 阶段时间窗 ------------------------------ */

  function phaseRow(p, i) {
    var row = document.createElement('div');
    row.className = 'row';
    row.style.marginBottom = '6px';

    var sel = document.createElement('select');
    PHASE_OPTIONS.forEach(function (ph) {
      var op = document.createElement('option');
      op.value = ph; op.textContent = ph;
      sel.appendChild(op);
    });
    sel.value = PHASE_OPTIONS.indexOf(p.phase) >= 0 ? p.phase : 'Phase0';
    sel.addEventListener('change', function () { state.phases[i].phase = sel.value; });
    row.appendChild(sel);

    var start = document.createElement('input');
    start.type = 'datetime-local';
    start.value = M.msToLocalInput(p.start_ms);
    row.appendChild(start);

    var end = document.createElement('input');
    end.type = 'datetime-local';
    end.value = M.msToLocalInput(p.end_ms);
    row.appendChild(end);

    var hint = document.createElement('span');
    hint.className = 'hint';
    function paintHint() {
      var s = M.localInputToMs(start.value) || p.start_ms;
      var e = M.localInputToMs(end.value) || p.end_ms;
      hint.textContent = (relDay(s) || '?') + ' → ' + (relDay(e) || '?');
    }
    start.addEventListener('input', function () {
      var ms = M.localInputToMs(start.value);
      if (ms == null) delete p.start_ms; else p.start_ms = ms;
      paintHint();
    });
    end.addEventListener('input', function () {
      var ms = M.localInputToMs(end.value);
      if (ms == null) delete p.end_ms; else p.end_ms = ms;
      paintHint();
    });
    paintHint();
    row.appendChild(hint);

    var del = document.createElement('button');
    del.className = 'danger small';
    del.textContent = '×';
    del.addEventListener('click', function () { state.phases.splice(i, 1); renderPhases(); });
    row.appendChild(del);
    return row;
  }

  function renderPhases() {
    var host = $('phases');
    if (!host) return;
    while (host.firstChild) host.removeChild(host.firstChild);
    for (var i = 0; i < state.phases.length; i++) host.appendChild(phaseRow(state.phases[i] || {}, i));
    if (!state.phases.length) {
      var e = document.createElement('div');
      e.className = 'hint';
      e.textContent = '暂无阶段时间窗（空 = 不限制）';
      host.appendChild(e);
    }
  }

  /* ------------------------------ 事件绑定 ------------------------------ */

  function bind() {
    $('api').value = apiBase();
    $('api').addEventListener('change', function (e) {
      var v = P.stripSlash(e.target.value) || P.DEFAULT_API;
      e.target.value = v;
      try { window.localStorage.setItem('paigu.display.api', v); } catch (err) { /* ignore */ }
      load().catch(function () {});
    });
    $('load').addEventListener('click', function () { load().catch(function () {}); });
    $('reload').addEventListener('click', reloadDisk);
    $('save').addEventListener('click', save);
    $('rounds-refresh').addEventListener('click', function () { loadRounds(); });
    $('rd-new-btn').addEventListener('click', createRound);
    $('phase-add').addEventListener('click', function () {
      var now = Date.now();
      state.phases.push({ phase: 'Phase0', start_ms: now, end_ms: now + 3600000 });
      renderPhases();
    });
    $('rd-start').addEventListener('input', updateWindowMs);
    $('rd-end').addEventListener('input', updateWindowMs);

    $('kind-tabs').addEventListener('click', function (e) {
      var b = e.target.closest ? e.target.closest('button[data-kind]') : null;
      if (!b) return;
      state.activeKind = b.getAttribute('data-kind');
      renderKindTabs();
      renderItems();
      paintConflicts();
    });
    $('alias-llm-btn').addEventListener('click', suggestAliases);
    $('item-add').addEventListener('click', function () { openTempAt($('item-add-end'), state.items.length, false); });
    $('item-add-end').addEventListener('click', function (e) {
      if (e.target.closest && e.target.closest('[data-act]')) return;
      openTempAt($('item-add-end'), state.items.length, false);
    });

    var items = $('items');
    items.addEventListener('input', onInput);
    items.addEventListener('change', onChange);
    items.addEventListener('click', onClick);
    items.addEventListener('keydown', onKeydown);
    items.addEventListener('focusout', onFocusOut);
  }

  function init() {
    bind();
    loadRounds();
    load().catch(function () {});
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
