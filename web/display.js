(function () {
  'use strict';

  var P = window.PAIGU;

  function $(id) { return document.getElementById(id); }

  var state = {
    source: 'local',
    apiBase: P.resolveApiBase(),
    remoteBase: P.resolveRemoteBase(),
    refreshMs: P.INLINE.refreshMs || 5000,
    roundId: P.qs('round') || '',
    version: 0,
    meta: null,
    config: null,
    items: [],
    messages: [],
    whoWhats: [],
    status: {},
    timer: null,
    inFlight: false,
    failures: 0,
    loaded: false,
    lastUpdate: 0
  };

  var boardEl, boardEmpty, messagesEl, whoEls, statusEl, boardRenderer, smart;

  var MESSAGE_STATUS_TEXT = {
    applied: '已记录',
    ok: '已记录',
    rejected: '已拒绝',
    reject: '已拒绝',
    needconfirm: '待确认',
    ignored: '已忽略',
    ignore: '已忽略',
    duplicate: '已去重',
    merged: '已合并'
  };

  var SYSTEM_LABELS = {
    bound_addr: '监听地址',
    clients: 'NapCat 连接',
    last_error: '最近错误',
    listening: '监听状态',
    status: '运行状态',
    version: '数据版本',
    updated_at: '更新时间'
  };

  function store(key, val) {
    try { window.localStorage.setItem('paigu.display.' + key, val); } catch (e) { /* ignore */ }
  }
  function readStore(key) {
    try { return window.localStorage.getItem('paigu.display.' + key); } catch (e) { return null; }
  }

  function showBanner(msg, bad) {
    var b = $('banner');
    b.textContent = msg;
    b.className = 'banner show' + (bad ? ' bad' : '');
  }
  function hideBanner() { $('banner').className = 'banner'; }

  function setConn(kind, text) {
    var el = $('conn-badge');
    el.className = 'status-badge ' + (kind || '');
    el.innerHTML = '<span class="dot"></span>' + P.esc(text);
  }

  function setVersion(v) {
    if (v == null) return;
    state.version = v;
    $('version-badge').textContent = '版本 #' + v;
  }

  function setRound(title) {
    if (title) $('round-title').textContent = title;
  }

  function countSlots(items) {
    var n = 0;
    for (var i = 0; i < items.length; i++) {
      for (var b = 0; b < items[i].boxes.length; b++) {
        var slots = items[i].boxes[b].slots;
        for (var s = 0; s < slots.length; s++) {
          if (slots[s].user && slots[s].status !== 'empty') n++;
        }
      }
      for (var g = 0; g < items[i].singles.length; g++) n += items[i].singles[g].quantity;
    }
    return n;
  }

  function renderBoard(items, changeMap) {
    boardRenderer.render(items, changeMap);
    $('board-count').textContent = items.length ? (items.length + ' 子项 · ' + countSlots(items) + ' 已排') : '';
    boardEmpty.hidden = items.length > 0;
  }

  function createMsg() {
    var el = document.createElement('div');
    el.className = 'msg';
    el.innerHTML = '<span class="seq"></span><span class="who"></span><span class="txt"></span><span class="st"></span><span class="detail"></span>';
    el.__seq = el.querySelector('.seq');
    el.__who = el.querySelector('.who');
    el.__txt = el.querySelector('.txt');
    el.__st = el.querySelector('.st');
    el.__detail = el.querySelector('.detail');
    el.addEventListener('click', function () {
      if (window.getSelection && window.getSelection().toString()) return;
      if (el.__editable && el.__msg) { el.focus({ preventScroll: true }); openOverride(el.__msg); }
    });
    el.addEventListener('keydown', function (e) {
      if (el.__editable && (e.key === 'Enter' || e.key === ' ')) {
        e.preventDefault(); openOverride(el.__msg);
      }
    });
    return el;
  }

  function openOverride(m) {
    if (state.source !== 'local' || !m.record_id || !m.record_revision || !m.round_id) {
      showBanner('此消息缺少可编辑的本地身份，请刷新后重试。', true); return;
    }
    P.openMessageEditor({ apiBase: state.apiBase, message: m, onSaved: function (res) {
      if (res && res.result && res.result.messages) renderMessages(P.normalizeMessages(res.result), true, false);
      restart(false);
    }});
  }

  function updateMsg(node, m) {
    var seqText = m.seq != null ? '#' + m.seq : '';
    if (node.__seq.textContent !== seqText) node.__seq.textContent = seqText;
    if (node.__who.textContent !== m.display) node.__who.textContent = m.display;
    if (node.__txt.textContent !== m.text) node.__txt.textContent = m.text;
    var st = m.status || '';
    var stKey = String(st).toLowerCase();
    var stText = MESSAGE_STATUS_TEXT[stKey] || st;
    if (node.__st.textContent !== stText) node.__st.textContent = stText;
    var stCls = 'st' + (st ? ' ' + stKey : '');
    if (node.__st.className !== stCls) node.__st.className = stCls;
    var detail = (m.detail || '') + (m.manual_note ? ' · 管理员解释：' + m.manual_note : '');
    if (node.__detail.textContent !== detail) node.__detail.textContent = detail;
    var nc = stKey === 'needconfirm';
    if (node.__needconfirm !== nc) {
      node.__needconfirm = nc;
      node.classList.toggle('needconfirm', nc);
    }
    node.__editable = !!(state.source === 'local' && m.record_id && m.record_revision && m.round_id);
    if (node.__editable) {
      node.tabIndex = 0;
      node.setAttribute('role', 'button');
      node.setAttribute('aria-label', '修订消息动作：' + m.display + '，' + m.text);
    } else {
      node.removeAttribute('tabindex'); node.removeAttribute('role'); node.removeAttribute('aria-label');
    }
    node.__msg = m;
  }

  function renderMessages(incoming, replace, initial) {
    var wasNear = smart.begin();
    var prevCount = state.messages.length;
    if (replace) {
      state.messages = incoming.slice();
    } else {
      var index = {};
      for (var i = 0; i < state.messages.length; i++) index[state.messages[i].key] = i;
      for (var j = 0; j < incoming.length; j++) {
        var m = incoming[j];
        if (index[m.key] != null) state.messages[index[m.key]] = m;
        else { state.messages.push(m); index[m.key] = state.messages.length - 1; }
      }
      state.messages.sort(function (a, b) {
        if (a.seq != null && b.seq != null) return a.seq - b.seq;
        return 0;
      });
    }
    P.syncKeyedChildren(messagesEl, state.messages, function (m) { return m.key; }, createMsg, updateMsg);
    $('msg-count').textContent = state.messages.length ? state.messages.length + ' 条' : '';
    var added = state.messages.length - prevCount;
    if (initial) smart.scrollToBottom();
    else smart.end(wasNear, added > 0 ? added : 0);
  }

  function renderWhoWhats(list) {
    P.syncKeyedChildren(whoEls, list, function (w) { return w.display; }, function () {
      var tr = document.createElement('tr');
      tr.innerHTML = '<td class="ww-display"></td><td class="ww-identity"></td><td class="ww-items"></td>';
      tr.__d = tr.querySelector('.ww-display');
      tr.__i = tr.querySelector('.ww-identity');
      tr.__it = tr.querySelector('.ww-items');
      return tr;
    }, function (tr, w) {
      if (tr.__d.textContent !== w.display) tr.__d.textContent = w.display;
      if (tr.__i.textContent !== w.identity) tr.__i.textContent = w.identity;
      var detail = w.items.map(function (it) { return it.name + ' ×' + it.qty; }).join('、');
      if (tr.__it.textContent !== detail) tr.__it.textContent = detail;
    });
    $('ww-count').textContent = list.length ? list.length + ' 人' : '';
    $('ww-empty').hidden = list.length > 0;
  }

  function createKv() {
    var wrap = document.createElement('div');
    wrap.style.display = 'contents';
    var k = document.createElement('div');
    k.className = 'k';
    var v = document.createElement('div');
    v.className = 'v';
    wrap.appendChild(k);
    wrap.appendChild(v);
    wrap.__kEl = k;
    wrap.__vEl = v;
    return wrap;
  }

  function renderStatus(status) {
    var rows = [];
    var keys = Object.keys(status || {});
    for (var i = 0; i < keys.length; i++) {
      var v = status[keys[i]];
      var text;
      if (v == null) text = '—';
      else if (Array.isArray(v)) text = v.map(function (x) { return typeof x === 'object' ? JSON.stringify(x) : String(x); }).join('、');
      else if (typeof v === 'object') text = JSON.stringify(v);
      else if (keys[i] === 'listening' && typeof v === 'boolean') text = v ? '监听中' : '未监听';
      else if (typeof v === 'boolean') text = v ? '是' : '否';
      else text = String(v);
      rows.push({ k: SYSTEM_LABELS[keys[i]] || keys[i], v: text });
    }
    if (!rows.length) {
      if (statusEl.__empty !== true) { statusEl.__empty = true; statusEl.textContent = '暂无状态'; }
      return;
    }
    if (statusEl.__empty) { statusEl.__empty = false; statusEl.textContent = ''; }
    P.syncKeyedChildren(statusEl, rows, function (r) { return r.k; }, createKv, function (node, r) {
      if (node.__kEl.textContent !== r.k) node.__kEl.textContent = r.k;
      if (node.__vEl.textContent !== r.v) node.__vEl.textContent = r.v;
    });
  }

  function markUpdated() {
    state.lastUpdate = Date.now();
    $('board-updated').textContent = '更新 ' + new Date(state.lastUpdate).toLocaleTimeString();
    $('poll-info').textContent = '每 ' + Math.round(state.refreshMs / 1000) + 's 轮询';
  }

  function applyPayload(payload) {
    if (!payload) return;
    if (state.loaded && payload.changed === false && payload.version != null && payload.version === state.version) {
      state.status = P.normalizeStatus(payload);
      renderStatus(state.status);
      return;
    }
    var nextItems = P.normalizeBoard(payload, state.meta);
    var changeMap = state.loaded ? P.normalizeChanged(payload) : {};
    var auto = state.loaded ? P.diffBoardItems(state.items, nextItems) : {};
    for (var k in auto) {
      if (!changeMap[k]) changeMap[k] = auto[k];
    }
    state.items = nextItems;
    renderBoard(nextItems, changeMap);

    if (payload.messages !== undefined) {
      renderMessages(P.normalizeMessages(payload), false, !state.loaded);
    }

    if (payload.who_whats !== undefined) {
      state.whoWhats = P.normalizeWhoWhats(payload);
      renderWhoWhats(state.whoWhats);
    } else if (!state.loaded) {
      state.whoWhats = P.deriveWhoWhats(nextItems);
      renderWhoWhats(state.whoWhats);
    }

    state.status = P.normalizeStatus(payload);
    renderStatus(state.status);
    setVersion(payload.version);
    state.loaded = true;
    markUpdated();
  }

  function applyRemote(snap) {
    if (!snap) return;
    var nextItems = P.normalizeBoard(snap, state.meta);
    var changeMap = state.loaded ? P.diffBoardItems(state.items, nextItems) : {};
    state.items = nextItems;
    renderBoard(nextItems, changeMap);

    if (snap.messages !== undefined) renderMessages(P.normalizeMessages(snap), true, !state.loaded);
    else if (!state.loaded) renderMessages([], true, true);

    var ww = P.normalizeWhoWhats(snap);
    if (!ww.length) ww = P.deriveWhoWhats(nextItems);
    state.whoWhats = ww;
    renderWhoWhats(ww);

    state.status = {
      round_id: snap.round_id,
      title: snap.title,
      status: snap.status,
      updated_at: snap.updated_at,
      warnings: (snap.warnings || []).length
    };
    renderStatus(state.status);
    setRound(snap.title);
    setVersion(snap.version);
    state.loaded = true;
    $('board-updated').textContent = '快照 ' + (snap.updated_at || '');
    $('poll-info').textContent = '每 ' + Math.round(state.refreshMs / 1000) + 's 轮询';
  }

  function schedule() {
    if (state.timer) { window.clearTimeout(state.timer); state.timer = null; }
    if (document.hidden) return;
    state.timer = window.setTimeout(pollOnce, state.refreshMs);
  }

  function pollOnce() {
    if (state.inFlight) return;

    if (state.source === 'remote') {
      if (!state.remoteBase) {
        showBanner('remote 数据源需要填写 Remote 地址（或在 HTML 内联配置 remoteBaseUrl）。', true);
        setConn('bad', '未配置远程');
        schedule();
        return;
      }
      state.inFlight = true;
      P.loadRemote(state.remoteBase, state.roundId).then(function (snap) {
        applyRemote(snap);
        state.failures = 0;
        hideBanner();
        setConn('ok', '远程快照');
      }).catch(function (err) {
        state.failures++;
        showBanner('远程快照加载失败：' + P.errorText(err) + '（重试中…）', true);
        setConn('bad', '远程失败');
      }).then(function () {
        state.inFlight = false;
        schedule();
      });
      return;
    }

    state.inFlight = true;
    P.get('/api/display?since=' + encodeURIComponent(state.version)).then(function (payload) {
      applyPayload(payload);
      state.failures = 0;
      hideBanner();
      setConn('ok', 'API 正常');
    }).catch(function (err) {
      state.failures++;
      showBanner('API 未就绪：' + P.errorText(err) + '。请确认本地服务已在 ' + state.apiBase + ' 运行（自动重试中…）', true);
      setConn('bad', state.failures > 3 ? '离线' : '连接中…');
    }).then(function () {
      state.inFlight = false;
      schedule();
    });
  }

  function restart(resetVersion) {
    if (resetVersion) {
      state.version = 0;
      state.items = [];
      state.messages = [];
      state.whoWhats = [];
      state.loaded = false;
      boardRenderer.clear();
      while (messagesEl.firstChild) messagesEl.removeChild(messagesEl.firstChild);
      while (whoEls.firstChild) whoEls.removeChild(whoEls.firstChild);
      smart.reset();
      $('version-badge').textContent = '版本 —';
      $('msg-count').textContent = '';
      $('board-count').textContent = '';
    }
    if (state.timer) { window.clearTimeout(state.timer); state.timer = null; }
    pollOnce();
  }

  function toggleSourceFields() {
    var remote = state.source === 'remote';
    $('api-wrap').classList.toggle('hidden', remote);
    $('remote-wrap').classList.toggle('hidden', !remote);
  }

  function loadConfig() {
    return P.get('/api/config').then(function (res) {
      var cfg = res && res.config ? res.config : res;
      if (!cfg) return;
      state.config = cfg;
      state.meta = P.buildMeta(cfg);
      var d = cfg.display || {};
      if (d.refresh_ms && !P.INLINE.refreshMs) state.refreshMs = d.refresh_ms;
      if (!P.resolveSource() && !readStore('source') && d.data_source) state.source = d.data_source;
      if (!P.resolveRemoteBase() && !readStore('remote') && d.remote_base_url) state.remoteBase = d.remote_base_url;
      if (state.meta.round && state.meta.round.round_id) state.roundId = state.meta.round.round_id;
      if (state.meta.round && state.meta.round.title) setRound(state.meta.round.title);
      $('remote').value = state.remoteBase;
      $('source').value = state.source;
      toggleSourceFields();
    }).catch(function () { /* config is optional for display */ });
  }

  function init() {
    boardEl = $('board');
    boardEmpty = $('board-empty');
    messagesEl = $('messages');
    whoEls = $('who-whats');
    statusEl = $('status');

    boardRenderer = P.createBoardRenderer(boardEl);
    P.boardLayout.bindControls($('board-width'));
    smart = P.createSmartScroll({ container: messagesEl, button: $('new-content'), threshold: 60 });
    renderStatus({});

    state.source = P.resolveSource() || readStore('source') || P.INLINE.dataSource || 'local';
    if (!P.qs('api')) {
      var storedApi = readStore('api');
      if (storedApi) state.apiBase = P.stripSlash(storedApi);
    }
    if (!P.qs('remote')) {
      var storedRemote = readStore('remote');
      if (storedRemote) state.remoteBase = P.stripSlash(storedRemote);
    }

    $('api').value = state.apiBase;
    $('remote').value = state.remoteBase;
    $('source').value = state.source;
    toggleSourceFields();

    $('source').addEventListener('change', function (e) {
      state.source = e.target.value;
      store('source', state.source);
      toggleSourceFields();
      restart(true);
    });
    $('api').addEventListener('change', function (e) {
      state.apiBase = P.stripSlash(e.target.value) || P.DEFAULT_API;
      e.target.value = state.apiBase;
      store('api', state.apiBase);
      restart(true);
    });
    $('remote').addEventListener('change', function (e) {
      state.remoteBase = P.stripSlash(e.target.value);
      store('remote', state.remoteBase);
      restart(true);
    });
    $('refresh').addEventListener('click', function () { restart(false); });

    document.addEventListener('visibilitychange', function () {
      if (document.hidden) {
        if (state.timer) { window.clearTimeout(state.timer); state.timer = null; }
      } else {
        pollOnce();
      }
    });

    loadConfig().then(function () {
      $('api').value = state.apiBase;
      restart(true);
    });
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
