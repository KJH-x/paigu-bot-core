(function (global) {
  'use strict';

  var DEFAULT_API = 'http://127.0.0.1:21081';
  var DEFAULT_WS = 'ws://127.0.0.1:9801';
  var INLINE = (global.PAIGU_WEB && typeof global.PAIGU_WEB === 'object') ? global.PAIGU_WEB : {};

  function qs(name) {
    var re = new RegExp('[?&]' + name + '=([^&#]*)');
    var m = re.exec(global.location && global.location.search ? global.location.search : '');
    return m ? decodeURIComponent(m[1].replace(/\+/g, ' ')) : null;
  }

  function stripSlash(s) {
    return String(s == null ? '' : s).replace(/\/+$/, '');
  }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function asArray(v) {
    if (v == null) return [];
    return Array.isArray(v) ? v : [v];
  }

  function num(v, d) {
    var n = parseInt(v, 10);
    return isNaN(n) ? d : n;
  }

  // Rust enums currently serialize as PascalCase. Normalize them at the UI
  // boundary so component state and CSS selectors use one stable vocabulary.
  function enumKey(v, fallback) {
    var s = String(v == null ? (fallback || '') : v).trim();
    return s
      .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
      .replace(/[\s-]+/g, '_')
      .toLowerCase();
  }

  function isObj(v) {
    return v != null && typeof v === 'object' && !Array.isArray(v);
  }

  function deepClone(v) {
    return v == null ? v : JSON.parse(JSON.stringify(v));
  }

  function cleanNickname(raw) {
    var s = String(raw == null ? '' : raw);
    var idx = s.search(/[（(]/);
    if (idx >= 0) s = s.slice(0, idx);
    return s.trim();
  }

  function resolveApiBase() {
    var v = qs('api');
    if (v) return stripSlash(v);
    if (INLINE.apiBase) return stripSlash(INLINE.apiBase);
    return DEFAULT_API;
  }

  function resolveWsBase() {
    var v = qs('ws');
    if (v) return String(v).trim();
    if (INLINE.wsUrl) return String(INLINE.wsUrl).trim();
    return DEFAULT_WS;
  }

  function resolveSource() {
    var v = qs('source');
    if (v === 'remote' || v === 'local') return v;
    if (INLINE.dataSource === 'remote' || INLINE.dataSource === 'local') return INLINE.dataSource;
    return null;
  }

  function resolveRemoteBase() {
    var v = qs('remote');
    if (v) return stripSlash(v);
    if (INLINE.remoteBaseUrl) return stripSlash(INLINE.remoteBaseUrl);
    return '';
  }

  function request(path, opts) {
    opts = opts || {};
    var url = /^https?:\/\//i.test(path) ? path : resolveApiBase() + path;
    var ctrl = typeof global.AbortController === 'function' ? new global.AbortController() : null;
    var timer = null;
    var init = {
      method: opts.method || 'GET',
      cache: 'no-store',
      headers: Object.assign({ Accept: 'application/json' }, opts.headers || {})
    };
    if (ctrl) {
      init.signal = ctrl.signal;
      timer = global.setTimeout(function () { ctrl.abort(); }, opts.timeout || 8000);
    }
    if (opts.body !== undefined) {
      init.headers['Content-Type'] = 'application/json';
      init.body = JSON.stringify(opts.body);
    }
    return global.fetch(url, init).then(function (res) {
      if (timer) global.clearTimeout(timer);
      return res.text().then(function (txt) {
        var data = null;
        if (txt) { try { data = JSON.parse(txt); } catch (e) { data = txt; } }
        if (!res.ok) {
          var err = new Error('HTTP ' + res.status + (res.statusText ? ' ' + res.statusText : ''));
          err.status = res.status;
          err.data = data;
          err.url = url;
          throw err;
        }
        return data;
      });
    }).catch(function (err) {
      if (timer) global.clearTimeout(timer);
      if (!err.status) {
        err.network = true;
        if (!err.url) err.url = url;
      }
      throw err;
    });
  }

  function get(path, opts) { return request(path, Object.assign({ method: 'GET' }, opts || {})); }
  function post(path, body, opts) { return request(path, Object.assign({ method: 'POST', body: body === undefined ? {} : body }, opts || {})); }
  function put(path, body, opts) { return request(path, Object.assign({ method: 'PUT', body: body }, opts || {})); }

  function errorText(err) {
    if (!err) return '未知错误';
    if (err.network) return '无法连接 ' + (err.url || resolveApiBase());
    if (err.status) return 'HTTP ' + err.status + (err.data && err.data.error ? '：' + err.data.error : '');
    return err.message || String(err);
  }

  function toast(msg, kind) {
    var host = document.getElementById('paigu-toast');
    if (!host) {
      host = document.createElement('div');
      host.id = 'paigu-toast';
      document.body.appendChild(host);
    }
    host.className = 'paigu-toast show' + (kind ? ' ' + kind : '');
    host.textContent = msg;
    if (host.__t) global.clearTimeout(host.__t);
    host.__t = global.setTimeout(function () { host.className = 'paigu-toast'; }, 2400);
  }

  function syncKeyedChildren(parent, desired, getKey, create, update) {
    var existing = {};
    var i, node;
    for (i = 0; i < parent.children.length; i++) {
      node = parent.children[i];
      if (node.__k != null) existing[node.__k] = node;
    }
    var seen = {};
    var prev = null;
    for (i = 0; i < desired.length; i++) {
      var item = desired[i];
      var k = getKey(item);
      seen[k] = true;
      node = existing[k];
      if (!node) {
        node = create(item, k);
        node.__k = k;
      }
      update(node, item, k);
      var target = prev ? prev.nextSibling : parent.firstChild;
      if (target !== node) parent.insertBefore(node, target);
      prev = node;
    }
    for (var k2 in existing) {
      if (Object.prototype.hasOwnProperty.call(existing, k2) && !seen[k2]) {
        parent.removeChild(existing[k2]);
      }
    }
  }

  function createBoardRenderer(host, options) {
    var changeMap = {};
    var persistent = !!(options && options.persistent);

    var REASON_META = {
      NewClaimFilled: { cls: 'changed-new', icon: '▲' },
      CancelReleased: { cls: 'changed-release', icon: '▼' },
      AutoMovedForward: { cls: 'changed-forward', icon: '↷' },
      TailSegmentCreated: { cls: 'changed-tail', icon: '⛓' },
      TailSegmentUpdated: { cls: 'changed-tail', icon: '⛓' },
      AdminFixed: { cls: 'changed-admin', icon: '✚' },
      AdminUnlocked: { cls: 'changed-admin', icon: '✚' },
      RecomputedByRuleChange: { cls: 'changed-forward', icon: '↻' }
    };

    function setCellClass(node) {
      var c = 'cell';
      c += node.__occupied ? ' filled' : ' empty';
      if (node.__locked) c += ' locked';
      if (node.__reserved) c += ' reserved';
      if (node.__changedCls) c += ' changed ' + node.__changedCls;
      if (node.className !== c) node.className = c;
    }

    function clearHighlight(node) {
      node.__changedCls = null;
      setCellClass(node);
      if (node.__badge) node.__badge.hidden = true;
    }

    function applyHighlight(node, info) {
      var meta = REASON_META[info && info.reason] ||
        (info && info.after ? REASON_META.NewClaimFilled : REASON_META.CancelReleased);
      node.__changedCls = meta.cls;
      setCellClass(node);
      if (node.__badge) {
        node.__badge.hidden = false;
        node.__badge.textContent = meta.icon;
      }
      var before = info && info.before != null ? info.before : '空';
      var after = info && info.after != null ? info.after : '空';
      node.title = (info && info.reason ? info.reason + '：' : '') + before + ' → ' + after;
      if (persistent) return;
      if (node.__hl) global.clearTimeout(node.__hl);
      node.__hl = global.setTimeout(function () { clearHighlight(node); }, 3000);
    }

    function createCell(cell, key) {
      var node = document.createElement('div');
      node.className = 'cell empty';
      var user = document.createElement('span');
      user.className = 'cell-user';
      node.appendChild(user);
      var badge = document.createElement('span');
      badge.className = 'badge';
      badge.hidden = true;
      node.appendChild(badge);
      node.__badge = badge;
      node.__user = user;
      return node;
    }

    function updateCell(node, cell) {
      var occupied = !!(cell.user && cell.status !== 'empty');
      node.__occupied = occupied;
      node.__locked = cell.status === 'locked' || cell.status === 'locked_empty';
      node.__reserved = cell.status === 'admin_reserved';
      setCellClass(node);
      var label = occupied ? cell.user : '';
      if (node.__user.textContent !== label) node.__user.textContent = label;
      if (!occupied) node.title = cell.status === 'locked_empty' ? '包尾锁定空位' : '';
    }

    function createRow() {
      var el = document.createElement('div');
      el.className = 'vrow';
      var label = document.createElement('div');
      label.className = 'vlabel';
      el.appendChild(label);
      var cells = document.createElement('div');
      cells.className = 'cells';
      el.appendChild(cells);
      var extra = document.createElement('div');
      extra.className = 'extra';
      el.appendChild(extra);
      el.__label = label;
      el.__cells = cells;
      el.__extra = extra;
      return el;
    }

    function updateRow(node, row) {
      var labelText = (row.variant_name || '单领') + (row.capacity ? '（' + row.capacity + '）' : '');
      if (node.__label.textContent !== labelText) node.__label.textContent = labelText;

      var desired = [];
      var multiBox = row.boxes.length > 1;
      for (var b = 0; b < row.boxes.length; b++) {
        var box = row.boxes[b];
        if (multiBox) desired.push({ kind: 'box', box_index: box.box_index });
        var cap = row.capacity != null ? row.capacity : 0;
        var maxIndex = cap;
        for (var s = 0; s < box.slots.length; s++) {
          if (box.slots[s].index > maxIndex) maxIndex = box.slots[s].index;
        }
        if (maxIndex < 1) maxIndex = 1;
        var byIndex = {};
        for (var s2 = 0; s2 < box.slots.length; s2++) byIndex[box.slots[s2].index] = box.slots[s2];
        for (var i = 1; i <= maxIndex; i++) {
          desired.push({
            kind: 'slot',
            box_index: box.box_index,
            index: i,
            user: byIndex[i] ? byIndex[i].user : '',
            status: byIndex[i] ? byIndex[i].status : 'empty',
            policy: byIndex[i] ? byIndex[i].policy : 'normal',
            segment_id: byIndex[i] ? byIndex[i].segment_id : null
          });
        }
      }

      var rowKey = row.key;
      syncKeyedChildren(
        node.__cells,
        desired,
        function (c) { return c.kind === 'box' ? 'box:' + c.box_index : rowKey + '#' + c.box_index + ':' + c.index; },
        function (c) {
          if (c.kind === 'box') {
            var t = document.createElement('span');
            t.className = 'box-tag';
            t.textContent = '盒' + c.box_index;
            return t;
          }
          return createCell(c);
        },
        function (n, c, k) {
          if (c.kind === 'box') return;
          updateCell(n, c);
          var info = changeMap[k];
          if (info) applyHighlight(n, info);
          else if (persistent) clearHighlight(n);
        }
      );

      var extra = '';
      if (row.singles.length) {
        extra += '<div class="singles">单领：' + row.singles.map(function (s) {
          return '<span class="chip">' + esc(s.display) + ' ×' + esc(s.quantity) + '</span>';
        }).join('') + '</div>';
      }
      if (row.waiting.length) {
        extra += '<div class="waiting">等待：' + row.waiting.map(function (w) {
          return '<span class="chip">' + esc(w.display) + ' ×' + esc(w.quantity) + '</span>';
        }).join('') + '</div>';
      }
      if (node.__extra.__html !== extra) {
        node.__extra.__html = extra;
        node.__extra.innerHTML = extra;
      }
    }

    function createSection() {
      var el = document.createElement('section');
      el.className = 'item-section';
      var title = document.createElement('div');
      title.className = 'item-title';
      el.appendChild(title);
      var rows = document.createElement('div');
      rows.className = 'rows';
      el.appendChild(rows);
      el.__title = title;
      el.__rows = rows;
      return el;
    }

    function updateSection(node, group) {
      var html = esc(group.name);
      if (group.kind) html += '<span class="kind">' + esc(group.kind) + '</span>';
      if (node.__title.__html !== html) {
        node.__title.__html = html;
        node.__title.innerHTML = html;
      }
      syncKeyedChildren(node.__rows, group.rows, function (r) { return r.key; }, createRow, updateRow);
    }

    function render(items, changes) {
      changeMap = changes || {};
      var groups = [];
      var groupMap = {};
      for (var i = 0; i < items.length; i++) {
        var it = items[i];
        var g = groupMap[it.item_id];
        if (!g) {
          g = { item_id: it.item_id, name: it.name, kind: it.kind, rows: [] };
          groupMap[it.item_id] = g;
          groups.push(g);
        }
        g.rows.push(it);
      }
      syncKeyedChildren(host, groups, function (g) { return g.item_id; }, createSection, updateSection);
    }

    function clear() {
      while (host.firstChild) host.removeChild(host.firstChild);
    }

    return { render: render, clear: clear };
  }

  function createSmartScroll(opts) {
    var container = opts.container;
    var threshold = opts.threshold == null ? 48 : opts.threshold;
    var button = opts.button || null;
    var unread = 0;
    var api = {};

    function nearBottom() {
      return (container.scrollHeight - container.scrollTop - container.clientHeight) <= threshold;
    }

    function renderButton() {
      if (!button) return;
      if (unread > 0) {
        button.hidden = false;
        button.textContent = '有新内容 ' + unread + ' 条';
      } else {
        button.hidden = true;
      }
    }

    api.isNearBottom = nearBottom;

    api.begin = function () { return nearBottom(); };

    api.end = function (wasNear, added) {
      if (wasNear) {
        api.scrollToBottom();
      } else if (added > 0) {
        unread += added;
        renderButton();
      }
    };

    api.scrollToBottom = function () {
      container.scrollTop = container.scrollHeight;
      unread = 0;
      renderButton();
    };

    api.reset = function () { unread = 0; renderButton(); };
    api.unread = function () { return unread; };

    if (button) {
      button.addEventListener('click', function () { api.scrollToBottom(); });
    }
    container.addEventListener('scroll', function () {
      if (nearBottom() && unread > 0) { unread = 0; renderButton(); }
    });

    return api;
  }

  function parseOffset(str) {
    if (str == null) return 0;
    var s = String(str).trim();
    if (!s) return 0;
    var sign = 1;
    if (s.charAt(0) === '-') { sign = -1; s = s.slice(1).trim(); }
    else if (s.charAt(0) === '+') { s = s.slice(1).trim(); }
    var parts = s.split(/[\s:]+/).filter(function (x) { return x !== ''; });
    if (!parts.length) return null;
    if (parts.length > 4) return null;
    var nums = [];
    for (var i = 0; i < parts.length; i++) {
      if (!/^\d+$/.test(parts[i])) return null;
      nums.push(parseInt(parts[i], 10));
    }
    while (nums.length < 4) nums.unshift(0);
    var dd = nums[0], hh = nums[1], mm = nums[2], ss = nums[3];
    if (mm > 59 || ss > 59) return null;
    var total = (((dd * 24 + hh) * 60 + mm) * 60 + ss) * 1000;
    return sign * total;
  }

  function formatOffset(ms) {
    var sign = ms < 0 ? '-' : '+';
    var t = Math.abs(Math.floor(ms / 1000));
    var dd = Math.floor(t / 86400); t -= dd * 86400;
    var hh = Math.floor(t / 3600); t -= hh * 3600;
    var mm = Math.floor(t / 60); t -= mm * 60;
    function p(n) { return (n < 10 ? '0' : '') + n; }
    return sign + p(dd) + ' ' + p(hh) + ' ' + p(mm) + ' ' + p(t);
  }

  function buildOneBotEvent(opts) {
    opts = opts || {};
    var offsetMs = num(opts.offsetMs != null ? opts.offsetMs : opts.offset_ms, 0) || 0;
    var text = String(opts.text == null ? '' : opts.text);
    var userId = opts.user_id != null ? String(opts.user_id) : '';
    var nickname = opts.nickname != null ? String(opts.nickname) : userId;
    var groupId = opts.group_id != null ? String(opts.group_id) : '';
    var messageId = opts.message_id != null ? String(opts.message_id) : ('sim-' + Date.now() + '-' + Math.floor(Math.random() * 1e6));
    var selfId = opts.self_id != null ? opts.self_id : 3000000000;
    var role = opts.is_admin ? 'admin' : 'member';
    return {
      post_type: 'message',
      message_type: 'group',
      self_id: selfId,
      user_id: userId,
      group_id: groupId,
      time: Math.floor((Date.now() + offsetMs) / 1000),
      message_id: messageId,
      raw_message: text,
      message: [{ type: 'text', data: { text: text } }],
      sender: { user_id: userId, nickname: nickname, card: nickname, role: role }
    };
  }

  function buildMeta(config) {
    var round = (config && config.round) || {};
    var items = asArray(round.items);
    var byId = {};
    var order = [];
    for (var i = 0; i < items.length; i++) {
      var it = items[i] || {};
      var id = it.item_id != null ? it.item_id : it.id;
      if (id == null) continue;
      id = String(id);
      var variants = {};
      var vorder = [];
      var vlist = asArray(it.variants);
      for (var v = 0; v < vlist.length; v++) {
        var vv = vlist[v] || {};
        var vid = vv.variant_id != null ? vv.variant_id : (vv.id != null ? vv.id : '');
        vid = String(vid);
        variants[vid] = { name: vv.name || vid, capacity: vv.capacity != null ? vv.capacity : null };
        vorder.push(vid);
      }
      byId[id] = {
        item_id: id,
        name: it.name || id,
        kind: it.kind || '',
        aliases: asArray(it.aliases),
        variants: variants,
        variant_order: vorder
      };
      order.push(id);
    }
    return {
      round: round,
      itemById: byId,
      order: order,
      priorityUsers: asArray(round.priority_users),
      priorityWindow: round.priority_window || null,
      display: (config && config.display) || {},
      members: (config && config.members) || {}
    };
  }

  function slotFromRaw(raw, fallbackIndex, boxIndex) {
    if (raw == null) return null;
    if (typeof raw === 'string') {
      return { index: fallbackIndex, box_index: boxIndex, user: raw, status: 'filled', policy: 'normal', segment_id: null };
    }
    var status = raw.status || raw.state || (raw.user || raw.display_name || raw.displayName || raw.nickname || raw.user_id ? 'filled' : 'empty');
    var user = raw.user || raw.display_name || raw.displayName || raw.nickname || raw.user_id || '';
    return {
      index: num(raw.slot_index != null ? raw.slot_index : (raw.slot != null ? raw.slot : raw.index), fallbackIndex),
      box_index: num(raw.box_index, boxIndex),
      user: user ? String(user) : '',
      status: enumKey(status, 'empty'),
      policy: enumKey(raw.policy || raw.slot_policy, 'normal'),
      segment_id: raw.segment_id || null
    };
  }

  function slotsFromContainer(container, boxIndex) {
    var out = [];
    if (container == null) return out;
    if (Array.isArray(container)) {
      for (var i = 0; i < container.length; i++) {
        var s = slotFromRaw(container[i], i + 1, boxIndex);
        if (s) out.push(s);
      }
      return out;
    }
    if (isObj(container)) {
      var keys = Object.keys(container);
      keys.sort(function (a, b) { return num(a, 0) - num(b, 0); });
      for (var k = 0; k < keys.length; k++) {
        var sl = slotFromRaw(container[keys[k]], num(keys[k], k + 1), boxIndex);
        if (sl) out.push(sl);
      }
    }
    return out;
  }

  function pickBoardSource(payload) {
    if (!payload) return null;
    if (payload.board !== undefined) return payload.board;
    if (Array.isArray(payload)) return payload;
    if (Array.isArray(payload.items) || Array.isArray(payload.item_allocations)) return payload;
    if (payload.version !== undefined || payload.messages !== undefined ||
        payload.status !== undefined || payload.who_whats !== undefined || payload.changed !== undefined) {
      return null;
    }
    return payload;
  }

  function normalizeBoard(payload, meta) {
    var src = pickBoardSource(payload);
    var items = [];
    var map = {};

    function itemMeta(itemId, variantId) {
      var m = meta && meta.itemById ? meta.itemById[itemId] : null;
      var name = m ? m.name : itemId;
      var kind = m ? m.kind : '';
      var vname = variantId || '';
      var capacity = null;
      if (m && m.variants && m.variants[variantId]) {
        vname = m.variants[variantId].name || variantId;
        capacity = m.variants[variantId].capacity;
      }
      return { name: name, kind: kind, variant_name: vname, capacity: capacity };
    }

    function pushRow(row) {
      if (map[row.key]) return;
      map[row.key] = row;
      items.push(row);
    }

    function rowFromItem(raw) {
      var itemId = raw.item_id != null ? String(raw.item_id) : (raw.id != null ? String(raw.id) : '');
      var variantId = raw.variant_id != null ? String(raw.variant_id) : '';
      var mm = itemMeta(itemId, variantId);
      var boxes = [];
      if (Array.isArray(raw.boxes) && raw.boxes.length) {
        for (var b = 0; b < raw.boxes.length; b++) {
          var box = raw.boxes[b] || {};
          var bi = num(box.box_index, b + 1);
          boxes.push({ box_index: bi, slots: slotsFromContainer(box.slots, bi) });
        }
      } else if (raw.slots != null) {
        boxes.push({ box_index: 1, slots: slotsFromContainer(raw.slots, 1) });
      } else {
        boxes.push({ box_index: 1, slots: [] });
      }
      return {
        key: itemId + '|' + variantId,
        item_id: itemId,
        name: raw.name || raw.item_name || mm.name,
        kind: raw.kind || mm.kind,
        variant_id: variantId,
        variant_name: raw.variant_name || mm.variant_name,
        capacity: raw.capacity != null ? raw.capacity : mm.capacity,
        boxes: boxes,
        singles: asArray(raw.singles).map(function (s) {
          return { display: s.display_name || s.display || s.user || '', quantity: num(s.quantity, 0) };
        }),
        waiting: asArray(raw.waiting).map(function (w) {
          return { display: w.display_name || w.display || w.user || '', quantity: num(w.quantity, 0), priority_level: num(w.priority_level, 0) };
        })
      };
    }

    if (Array.isArray(src)) {
      for (var i = 0; i < src.length; i++) pushRow(rowFromItem(src[i] || {}));
    } else if (isObj(src) && Array.isArray(src.items)) {
      for (var j = 0; j < src.items.length; j++) pushRow(rowFromItem(src.items[j] || {}));
    } else if (isObj(src) && Array.isArray(src.item_allocations)) {
      for (var k = 0; k < src.item_allocations.length; k++) pushRow(rowFromItem(src.item_allocations[k] || {}));
    } else if (isObj(src)) {
      var keys = Object.keys(src);
      for (var n = 0; n < keys.length; n++) {
        var key = keys[n];
        var parts = key.split('|');
        var itemId2 = parts[0];
        var variantId2 = parts.length > 1 ? parts.slice(1).join('|') : '';
        var mm2 = itemMeta(itemId2, variantId2);
        pushRow({
          key: itemId2 + '|' + variantId2,
          item_id: itemId2,
          name: mm2.name,
          kind: mm2.kind,
          variant_id: variantId2,
          variant_name: mm2.variant_name,
          capacity: mm2.capacity,
          boxes: [{ box_index: 1, slots: slotsFromContainer(src[key], 1) }],
          singles: [],
          waiting: []
        });
      }
    }

    if (meta && meta.order) {
      var ordered = [];
      var seen = {};
      for (var o = 0; o < meta.order.length; o++) {
        var m = meta.itemById[meta.order[o]];
        var vids = (m.variant_order && m.variant_order.length) ? m.variant_order : [''];
        var baseKey = m.item_id + '|';
        if (map[baseKey] && vids[0] !== '' && !seen[baseKey]) {
          ordered.push(map[baseKey]);
          seen[baseKey] = true;
        }
        for (var vi = 0; vi < vids.length; vi++) {
          var kk = m.item_id + '|' + vids[vi];
          if (seen[kk]) continue;
          ordered.push(map[kk] || rowFromItem({ item_id: m.item_id, variant_id: vids[vi] }));
          seen[kk] = true;
        }
      }
      for (var p = 0; p < items.length; p++) {
        if (!seen[items[p].key]) ordered.push(items[p]);
      }
      items = ordered;
    }

    return items;
  }

  function normalizeMessages(payload) {
    var src = payload && payload.messages !== undefined ? payload.messages : [];
    var out = [];
    for (var i = 0; i < asArray(src).length; i++) {
      var m = src[i] || {};
      var seq = m.seq != null ? m.seq : (m.sequence != null ? m.sequence : null);
      var id = m.message_id != null ? m.message_id : (m.id != null ? m.id : seq);
      var key = seq != null ? 's' + seq : ('m' + (id != null ? id : i));
      out.push({
        key: key,
        seq: seq,
        user: m.user || m.user_id || m.identity || '',
        display: m.display || m.display_name || m.nickname || m.user || m.user_id || '',
        text: m.text || m.raw_message || m.message || '',
        status: m.status || m.outcome || '',
        detail: m.detail || m.reason || '',
        ts: m.timestamp_ms != null ? m.timestamp_ms : (m.ts != null ? m.ts : null)
      });
    }
    return out;
  }

  function normalizeWhoWhats(payload) {
    var src = payload && payload.who_whats !== undefined ? payload.who_whats : [];
    var out = [];
    for (var i = 0; i < asArray(src).length; i++) {
      var w = src[i] || {};
      out.push({
        display: w.display || w.nickname || w.name || w.user || '',
        identity: w.identity || w.user_id || '',
        items: asArray(w.items).map(function (it) {
          return { name: it.name || it.item_name || '', qty: num(it.qty != null ? it.qty : it.quantity, 0) };
        })
      });
    }
    return out;
  }

  function deriveWhoWhats(items) {
    var byDisplay = {};
    var order = [];
    function add(display, name, qty) {
      if (!display) return;
      if (!byDisplay[display]) { byDisplay[display] = { display: display, identity: '', items: [], _m: {} }; order.push(display); }
      var rec = byDisplay[display];
      var k = name;
      if (!rec._m[k]) { rec._m[k] = { name: name, qty: 0 }; rec.items.push(rec._m[k]); }
      rec._m[k].qty += qty;
    }
    for (var i = 0; i < items.length; i++) {
      var it = items[i];
      for (var b = 0; b < it.boxes.length; b++) {
        var slots = it.boxes[b].slots;
        for (var s = 0; s < slots.length; s++) {
          if (slots[s].user && slots[s].status !== 'empty') add(slots[s].user, it.variant_name || it.name, 1);
        }
      }
      for (var g = 0; g < it.singles.length; g++) add(it.singles[g].display, it.name, it.singles[g].quantity);
      for (var w = 0; w < it.waiting.length; w++) add(it.waiting[w].display, it.name + '（等待）', it.waiting[w].quantity);
    }
    return order.map(function (d) { delete byDisplay[d]._m; return byDisplay[d]; });
  }

  function normalizeStatus(payload) {
    var s = payload ? payload.status : null;
    if (s == null) return {};
    if (typeof s === 'string') return { status: s };
    if (isObj(s)) return s;
    return {};
  }

  function normalizeMembers(payload) {
    var src = payload;
    if (isObj(payload)) {
      src = payload.members != null ? payload.members : (payload.data != null ? payload.data : payload);
    }
    var out = [];
    for (var i = 0; i < asArray(src).length; i++) {
      var m = src[i] || {};
      if (typeof m === 'string') { out.push({ user_id: m, nickname: m, role: '', cleaned: cleanNickname(m) }); continue; }
      var nick = m.nickname || m.card || m.name || m.display_name || m.user_id || '';
      out.push({
        user_id: m.user_id != null ? String(m.user_id) : (m.id != null ? String(m.id) : nick),
        nickname: String(nick),
        role: m.role || m.role_name || '',
        cleaned: cleanNickname(nick),
        raw: m
      });
    }
    return out;
  }

  var EMBEDDED_MEMBERS = [
    '成员01', '成员02', '成员03', '成员04', '成员05'
  ];

  var PRIORITY_USERS = ['user_a', 'user_b', 'user_c', 'user_d'];

  function embeddedMembers() {
    return EMBEDDED_MEMBERS.map(function (n) {
      return { user_id: n, nickname: n, role: '', cleaned: n, fallback: true };
    });
  }

  function diffBoardItems(prevItems, nextItems) {
    var prev = {};
    for (var i = 0; i < prevItems.length; i++) {
      var it = prevItems[i];
      for (var b = 0; b < it.boxes.length; b++) {
        var slots = it.boxes[b].slots;
        for (var s = 0; s < slots.length; s++) {
          prev[it.key + '#' + slots[s].box_index + ':' + slots[s].index] = slots[s];
        }
      }
    }
    var changed = {};
    for (var j = 0; j < nextItems.length; j++) {
      var nit = nextItems[j];
      for (var bb = 0; bb < nit.boxes.length; bb++) {
        var nslots = nit.boxes[bb].slots;
        for (var ss = 0; ss < nslots.length; ss++) {
          var sl = nslots[ss];
          var ck = nit.key + '#' + sl.box_index + ':' + sl.index;
          var old = prev[ck];
          if (!old || old.user !== sl.user || old.status !== sl.status) {
            changed[ck] = { before: old ? old.user : null, after: sl.user || null };
          }
        }
      }
    }
    return changed;
  }

  function normalizeChanged(payload) {
    var src = payload ? payload.changed : null;
    var map = {};
    if (!src || src === true) return map;
    if (typeof src === 'boolean') return map;
    var list = Array.isArray(src) ? src : (isObj(src) && Array.isArray(src.changes) ? src.changes : null);
    if (list) {
      for (var i = 0; i < list.length; i++) {
        var c = list[i] || {};
        var itemId = c.item_id != null ? String(c.item_id) : '';
        var variantId = c.variant_id != null ? String(c.variant_id) : '';
        var boxIndex = num(c.box_index, 1);
        var slotIndex = num(c.slot_index != null ? c.slot_index : c.slot, 0);
        if (!itemId || !slotIndex) continue;
        map[itemId + '|' + variantId + '#' + boxIndex + ':' + slotIndex] = {
          before: c.before != null ? c.before : null,
          after: c.after != null ? c.after : null,
          reason: c.reason || ''
        };
      }
      return map;
    }
    if (isObj(src)) {
      var keys = Object.keys(src);
      for (var k = 0; k < keys.length; k++) {
        var arr = asArray(src[keys[k]]);
        for (var a = 0; a < arr.length; a++) {
          var cc = arr[a] || {};
          var si = num(cc.slot_index != null ? cc.slot_index : cc.slot, 0);
          if (!si) continue;
          map[keys[k] + '#' + num(cc.box_index, 1) + ':' + si] = {
            before: cc.before != null ? cc.before : null,
            after: cc.after != null ? cc.after : null,
            reason: cc.reason || ''
          };
        }
      }
    }
    return map;
  }

  function resolveRemoteCandidates(remoteBase, roundId) {
    var base = stripSlash(remoteBase);
    if (!base) return [];
    if (/\.json(\?|$)/i.test(base)) return [base];
    var list = [];
    if (roundId) list.push(base + '/rounds/' + encodeURIComponent(roundId) + '/current');
    list.push(base + '/current');
    return list;
  }

  function loadRemote(remoteBase, roundId) {
    var candidates = resolveRemoteCandidates(remoteBase, roundId);
    var i = 0;
    function attempt() {
      if (i >= candidates.length) {
        var e = new Error('未找到远程快照');
        e.network = true;
        e.url = remoteBase;
        throw e;
      }
      var url = candidates[i++];
      return global.fetch(url, { cache: 'no-store' }).then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      }).catch(function (err) {
        if (i < candidates.length) return attempt();
        throw err;
      });
    }
    return attempt();
  }

  global.PAIGU = {    DEFAULT_API: DEFAULT_API,
    DEFAULT_WS: DEFAULT_WS,
    INLINE: INLINE,
    EMBEDDED_MEMBERS: EMBEDDED_MEMBERS,
    PRIORITY_USERS: PRIORITY_USERS,
    qs: qs,
    esc: esc,
    asArray: asArray,
    num: num,
    isObj: isObj,
    deepClone: deepClone,
    cleanNickname: cleanNickname,
    stripSlash: stripSlash,
    resolveApiBase: resolveApiBase,
    resolveWsBase: resolveWsBase,
    resolveSource: resolveSource,
    resolveRemoteBase: resolveRemoteBase,
    request: request,
    get: get,
    post: post,
    put: put,
    errorText: errorText,
    toast: toast,
    syncKeyedChildren: syncKeyedChildren,
    createBoardRenderer: createBoardRenderer,
    createSmartScroll: createSmartScroll,
    parseOffset: parseOffset,
    formatOffset: formatOffset,
    buildOneBotEvent: buildOneBotEvent,
    buildMeta: buildMeta,
    pickBoardSource: pickBoardSource,
    normalizeBoard: normalizeBoard,
    normalizeMessages: normalizeMessages,
    normalizeWhoWhats: normalizeWhoWhats,
    deriveWhoWhats: deriveWhoWhats,
    normalizeStatus: normalizeStatus,
    normalizeMembers: normalizeMembers,
    embeddedMembers: embeddedMembers,
    diffBoardItems: diffBoardItems,
    normalizeChanged: normalizeChanged,
    resolveRemoteCandidates: resolveRemoteCandidates,
    loadRemote: loadRemote
  };
})(window);
