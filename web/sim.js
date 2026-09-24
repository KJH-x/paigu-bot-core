(function () {
  'use strict';

  var P = window.PAIGU;

  function $(id) { return document.getElementById(id); }

  var state = {
    apiBase: P.resolveApiBase(),
    wsUrl: P.resolveWsBase(),
    refreshMs: P.INLINE.refreshMs || 3000,
    version: 0,
    lastSeq: 0,
    meta: null,
    config: null,
    groupId: '',
    items: [],
    turns: [],
    turnSeq: 0,
    members: [],
    identity: null,
    timer: null,
    wsTimer: null,
    inFlight: false,
    failures: 0,
    ws: null,
    wsReady: false,
    wsRetry: 0,
    msgCounter: 0,
    customSeq: 0
  };

  var boardRenderer;

  function store(key, val) {
    try { window.localStorage.setItem('paigu.sim.' + key, val); } catch (e) { /* ignore */ }
  }
  function readStore(key) {
    try { return window.localStorage.getItem('paigu.sim.' + key); } catch (e) { return null; }
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

  function setWs(kind, text) {
    var el = $('ws-badge');
    el.className = 'status-badge ' + (kind || '');
    el.innerHTML = '<span class="dot"></span>' + P.esc(text);
  }

  function setVersion(v) {
    if (v == null) return;
    state.version = v;
    $('version-badge').textContent = '版本 #' + v;
  }

  function countSlots(items) {
    var n = 0;
    for (var i = 0; i < items.length; i++) {
      for (var b = 0; b < items[i].boxes.length; b++) {
        var slots = items[i].boxes[b].slots;
        for (var s = 0; s < slots.length; s++) if (slots[s].user && slots[s].status !== 'empty') n++;
      }
      for (var g = 0; g < items[i].singles.length; g++) n += items[i].singles[g].quantity;
    }
    return n;
  }

  function renderBoard(items, changeMap) {
    boardRenderer.render(items, changeMap);
    $('board-count').textContent = items.length ? (items.length + ' 变体 · ' + countSlots(items) + ' 已排') : '';
    $('board-empty').hidden = items.length > 0;
  }

  function applyBoardPayload(payload) {
    if (!payload || payload.board === undefined) return;
    var next = P.normalizeBoard(payload, state.meta);
    var changeMap = P.normalizeChanged(payload);
    var auto = P.diffBoardItems(state.items, next);
    for (var k in auto) if (!changeMap[k]) changeMap[k] = auto[k];
    state.items = next;
    renderBoard(next, changeMap);
  }

  function outcomeInfo(outcome) {
    if (outcome == null) return { cls: '', text: '' };
    if (typeof outcome === 'string') return { cls: outcome.toLowerCase(), text: outcome };
    var status = outcome.status || outcome.outcome || outcome.result || '';
    var parts = [];
    if (status) parts.push(status);
    if (outcome.intent) parts.push('intent=' + outcome.intent);
    if (outcome.reply) parts.push(outcome.reply);
    if (outcome.detail) parts.push(outcome.detail);
    if (outcome.reason) parts.push(outcome.reason);
    if (outcome.confidence != null) parts.push('conf=' + outcome.confidence);
    if (outcome.items) parts.push(JSON.stringify(outcome.items));
    return { cls: String(status).toLowerCase(), text: parts.join(' · ') || JSON.stringify(outcome) };
  }

  function createTurn() {
    var el = document.createElement('div');
    el.className = 'turn';
    el.innerHTML = '<div class="head"><span class="who"></span><span class="off"></span><span class="seq"></span></div><div class="body"></div><div class="out"></div>';
    el.__who = el.querySelector('.who');
    el.__off = el.querySelector('.off');
    el.__seq = el.querySelector('.seq');
    el.__body = el.querySelector('.body');
    el.__out = el.querySelector('.out');
    return el;
  }

  function updateTurn(el, t) {
    if (el.__who.textContent !== t.display) el.__who.textContent = t.display;
    var off = t.offsetText ? '偏移 ' + t.offsetText : '';
    if (el.__off.textContent !== off) el.__off.textContent = off;
    var seq = t.pending ? '发送中…' : (t.seq != null ? '#' + t.seq : '');
    if (el.__seq.textContent !== seq) el.__seq.textContent = seq;
    if (el.__body.textContent !== t.text) el.__body.textContent = t.text;
    var out = outcomeInfo(t.outcome);
    if (el.__out.textContent !== out.text) el.__out.textContent = out.text;
    var cls = 'out' + (out.cls ? ' ' + out.cls : '') + (t.pending ? ' pending' : '');
    if (el.__out.className !== cls) el.__out.className = cls;
  }

  function renderTurns() {
    P.syncKeyedChildren($('transcript'), state.turns, function (t) { return t.key; }, createTurn, updateTurn);
    $('turn-count').textContent = state.turns.length ? state.turns.length + ' 条' : '';
    $('transcript-empty').hidden = state.turns.length > 0;
  }

  function pushTurn(text, outcome, version, offsetText, messageId) {
    state.turnSeq++;
    state.turns.push({
      key: 't' + state.turnSeq,
      display: state.identity ? (state.identity.nickname || state.identity.user_id) : '?',
      text: text,
      offsetText: offsetText,
      outcome: outcome,
      version: version,
      seq: null,
      pending: !!(outcome && outcome.status === 'sent'),
      messageId: messageId || null
    });
    renderTurns();
  }

  function addServerTurn(m) {
    state.turnSeq++;
    state.turns.push({
      key: 't' + state.turnSeq,
      display: m.display || m.user || m.user_id || '?',
      text: m.text || '',
      offsetText: '',
      outcome: { status: m.status || '', detail: m.detail || '' },
      version: null,
      seq: m.seq != null ? m.seq : null,
      pending: false,
      messageId: m.message_id || null
    });
  }

  function hasPending() {
    for (var i = 0; i < state.turns.length; i++) if (state.turns[i].pending) return true;
    return false;
  }

  function reconcileMessages(msgs) {
    if (!Array.isArray(msgs) || !msgs.length) return;
    var changed = false;
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i] || {};
      var seq = m.seq != null ? m.seq : null;
      if (seq != null && seq > state.lastSeq) state.lastSeq = seq;
      var matched = null;
      for (var j = 0; j < state.turns.length; j++) {
        var t = state.turns[j];
        if (t.pending && t.text === (m.text || '') && (!m.display || t.display === m.display)) { matched = t; break; }
      }
      if (matched) {
        matched.pending = false;
        matched.outcome = { status: m.status || '', detail: m.detail || '' };
        matched.seq = seq;
      } else {
        addServerTurn(m);
      }
      changed = true;
    }
    if (changed) renderTurns();
  }

  function pushWsReply(text) {
    var host = $('ws-replies');
    if (!host) return;
    var line = document.createElement('div');
    line.textContent = '服务端：' + text;
    host.appendChild(line);
    while (host.childNodes.length > 5) host.removeChild(host.firstChild);
  }

  function handleWsFrame(data) {
    var val = null;
    try { val = JSON.parse(data); } catch (e) { /* non-json */ }
    if (val && (val.action || val.echo)) return;
    pushWsReply(val ? JSON.stringify(val) : String(data));
  }

  function closeWs() {
    if (state.wsTimer) { window.clearTimeout(state.wsTimer); state.wsTimer = null; }
    if (state.ws) {
      try { state.ws.onclose = null; state.ws.close(); } catch (e) { /* ignore */ }
      state.ws = null;
    }
    state.wsReady = false;
    window.__simWsReady = false;
  }

  function scheduleWsReconnect() {
    if (state.wsTimer) return;
    // 有限重试：失败过多则停止自动重连，避免控制台噪声，改为提示手动「重连 WS」
    if (state.wsRetry >= 3) {
      setWs('bad', 'WS 未连接（已停止自动重连）');
      showBanner('WS 未连接：' + state.wsUrl + '。请确认网关在线，或点击「重连 WS」。', true);
      return;
    }
    var delay = Math.min(8000, 1000 * (state.wsRetry++));
    state.wsTimer = window.setTimeout(function () {
      state.wsTimer = null;
      connectWs();
    }, delay);
  }

  /** 解析 WS 目标：网关绑定在主机网卡（常非 127.0.0.1），优先采用后端实际 bind 地址。 */
  function resolveWsTarget() {
    if (P.qs('ws') || P.INLINE.wsUrl) return Promise.resolve();
    return P.get('/api/workflow')
      .then(function (wf) {
        var bound = wf && wf.gateway && wf.gateway.bound_addr;
        if (bound && bound.indexOf(':') > 0 && bound.indexOf('0.0.0.0') !== 0) {
          state.wsUrl = 'ws://' + bound;
          if ($('ws')) $('ws').value = state.wsUrl;
        }
      })
      .catch(function () { /* 接口不可用时沿用默认 */ });
  }

  function connectWs() {
    closeWs();
    var url = state.wsUrl;
    var ws;
    try { ws = new WebSocket(url); } catch (e) {
      setWs('bad', 'WS 地址无效');
      showBanner('WS 地址无效：' + url, true);
      return;
    }
    state.ws = ws;
    setWs('', 'WS 连接中…');
    ws.onopen = function () {
      state.wsReady = true;
      state.wsRetry = 0;
      window.__simWsReady = true;
      setWs('ok', 'WS 已连接');
      hideBanner();
    };
    ws.onmessage = function (ev) { handleWsFrame(ev.data); };
    ws.onerror = function () { /* onclose will follow */ };
    ws.onclose = function () {
      if (state.ws !== ws) return;
      state.wsReady = false;
      window.__simWsReady = false;
      setWs('bad', 'WS 未连接');
      scheduleWsReconnect();
    };
  }

  function currentOffsetMs() {
    return P.parseOffset($('offset').value);
  }

  function updateOffsetHint() {
    var ms = currentOffsetMs();
    var hint = $('offset-hint');
    if (ms === null) {
      hint.textContent = '格式无效，请用 ±DD HH MM SS（例：+00 00 05 00 = 延后 5 分钟）';
      hint.style.color = 'var(--danger)';
    } else {
      hint.textContent = '= ' + ms + ' ms' + (ms === 0 ? '（不偏移）' : '');
      hint.style.color = '';
    }
  }

  function priorityList() {
    var fromMeta = (state.meta && state.meta.priorityUsers) || [];
    return fromMeta.length ? fromMeta : P.PRIORITY_USERS;
  }

  function isPlaceholderName(name) {
    var s = P.cleanNickname(name || '').trim();
    if (!s) return false;
    return /^(成员\d+|模拟成员\d*|测试\d*|user_[a-z0-9_]+)$/i.test(s);
  }

  function buildIdentitySelect() {
    var sel = $('identity-select');
    var priorities = priorityList();
    while (sel.firstChild) sel.removeChild(sel.firstChild);
    for (var i = 0; i < state.members.length; i++) {
      var m = state.members[i];
      var opt = document.createElement('option');
      opt.value = m.user_id;
      opt.textContent = m.cleaned + (m.fallback ? '（内置占位）' : '') + (priorities.indexOf(m.cleaned) >= 0 ? ' · 预存' : '');
      sel.appendChild(opt);
    }
    var custom = document.createElement('option');
    custom.value = '__custom__';
    custom.textContent = '＋ 新建占位身份';
    sel.appendChild(custom);
  }

  function setIdentity(member) {
    var isPriority = priorityList().indexOf(P.cleanNickname(member.nickname || member.cleaned || '')) >= 0;
    state.identity = {
      user_id: member.user_id,
      nickname: member.cleaned || P.cleanNickname(member.nickname) || member.user_id,
      is_admin: !!member.is_admin,
      priority: isPriority,
      priority_level: isPriority ? 10 : 0
    };
    $('identity-info').textContent = state.identity.nickname + ' · ' + state.identity.user_id + (isPriority ? ' · 预存' : '');
    postIdentity();
  }

  function setCustomIdentity() {
    var userId = $('c-user').value.trim();
    var nick = $('c-nick').value.trim();
    if (!userId) { P.toast('请填写 user_id', 'bad'); return; }
    if (!nick) {
      state.customSeq++;
      nick = '模拟成员' + (state.customSeq < 10 ? '0' + state.customSeq : state.customSeq);
    }
    if (!isPlaceholderName(nick)) {
      P.toast('请使用占位名（如 成员06 / 模拟成员06），禁止真实昵称', 'bad');
      return;
    }
    state.identity = {
      user_id: userId,
      nickname: nick,
      is_admin: $('c-admin').checked,
      priority: $('c-priority').checked,
      priority_level: $('c-priority').checked ? (P.num($('c-level').value, 10) || 10) : 0
    };
    $('identity-info').textContent = state.identity.nickname + ' · ' + state.identity.user_id + (state.identity.priority ? ' · 预存' : '');
    postIdentity();
    P.toast('已切换为占位身份 ' + state.identity.nickname);
  }

  function postIdentity() {
    if (!state.identity) return;
    P.post('/api/sim/identity', {
      user_id: state.identity.user_id,
      nickname: state.identity.nickname,
      is_admin: state.identity.is_admin,
      priority: state.identity.priority ? 10 : 0,
      priority_level: state.identity.priority_level
    }).catch(function () { /* identity record is optional */ });
  }

  function send() {
    if (!state.identity) { P.toast('请先选择身份', 'bad'); return; }
    var text = $('text').value.trim();
    if (!text) { P.toast('请输入消息内容', 'bad'); return; }
    var ms = currentOffsetMs();
    if (ms === null) { P.toast('时间偏移格式无效', 'bad'); return; }
    if (!state.wsReady || !state.ws || state.ws.readyState !== 1) {
      showBanner('WS 未连接（' + state.wsUrl + '），无法发送。本页不提供 HTTP 旁路，请检查 WS 地址或点「重连 WS」。', true);
      P.toast('WS 未连接', 'bad');
      return;
    }
    var gid = $('group').value.trim() || state.groupId || '123456789';
    state.msgCounter++;
    var messageId = 'sim-' + Date.now() + '-' + state.msgCounter;
    var event = P.buildOneBotEvent({
      user_id: state.identity.user_id,
      nickname: state.identity.nickname,
      text: text,
      group_id: gid,
      offset_ms: ms,
      is_admin: state.identity.is_admin,
      message_id: messageId
    });
    try {
      state.ws.send(JSON.stringify(event));
    } catch (e) {
      showBanner('WS 发送失败：' + P.errorText(e), true);
      return;
    }
    pushTurn(text, { status: 'sent', detail: '已通过 WS 发送，等待服务端…' }, null, P.formatOffset(ms), messageId);
    $('text').value = '';
    hideBanner();
    pollAfterSend();
  }

  function pollAfterSend() {
    var tries = 0;
    function tick() {
      tries++;
      pollOnce();
      if (tries < 30 && hasPending()) window.setTimeout(tick, 200);
    }
    window.setTimeout(tick, 100);
  }

  function reset() {
    $('reset').disabled = true;
    P.post('/api/sim/reset', {}).then(function () {
      state.turns = [];
      state.turnSeq = 0;
      state.items = [];
      state.version = 0;
      state.lastSeq = 0;
      renderTurns();
      boardRenderer.clear();
      $('board-count').textContent = '';
      $('board-empty').hidden = false;
      $('version-badge').textContent = '版本 —';
      hideBanner();
      P.toast('模拟会话已重置', 'ok');
      pollOnce();
    }).catch(function (err) {
      showBanner('重置失败：' + P.errorText(err), true);
    }).then(function () {
      $('reset').disabled = false;
    });
  }

  function schedule() {
    if (state.timer) { window.clearTimeout(state.timer); state.timer = null; }
    if (document.hidden) return;
    state.timer = window.setTimeout(pollOnce, state.refreshMs);
  }

  function pollOnce() {
    if (state.inFlight) return;
    state.inFlight = true;
    P.get('/api/display?since=' + encodeURIComponent(state.lastSeq)).then(function (payload) {
      if (payload && payload.board !== undefined) applyBoardPayload(payload);
      if (payload && payload.messages) reconcileMessages(payload.messages);
      if (payload && payload.version != null) setVersion(payload.version);
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

  function loadMembers() {
    return P.get('/api/members').then(function (res) {
      var all = P.normalizeMembers(res);
      var list = [];
      for (var i = 0; i < all.length; i++) {
        if (isPlaceholderName(all[i].cleaned || all[i].nickname)) list.push(all[i]);
      }
      state.members = list.length ? list : P.embeddedMembers();
      buildIdentitySelect();
    }).catch(function () {
      state.members = P.embeddedMembers();
      buildIdentitySelect();
    });
  }

  function loadConfig() {
    return P.get('/api/config').then(function (res) {
      var cfg = res && res.config ? res.config : res;
      if (!cfg) return;
      state.config = cfg;
      state.meta = P.buildMeta(cfg);
      var d = cfg.display || {};
      if (d.refresh_ms && !P.INLINE.refreshMs) state.refreshMs = d.refresh_ms;
      if (cfg.round && cfg.round.group_id) {
        state.groupId = cfg.round.group_id;
        $('group').value = cfg.round.group_id;
      }
      if (cfg.members && cfg.members.group_id && !$('group').value) $('group').value = cfg.members.group_id;
    }).catch(function () { /* config optional */ });
  }

  function init() {
    window.__simWsReady = false;
    boardRenderer = P.createBoardRenderer($('board'));

    if (!P.qs('api')) {
      var storedApi = readStore('api');
      if (storedApi) state.apiBase = P.stripSlash(storedApi);
    }
    $('api').value = state.apiBase;
    $('api').addEventListener('change', function (e) {
      state.apiBase = P.stripSlash(e.target.value) || P.DEFAULT_API;
      e.target.value = state.apiBase;
      store('api', state.apiBase);
      P.toast('API 地址已更新');
    });

    if (!P.qs('ws')) {
      var storedWs = readStore('ws');
      if (storedWs) state.wsUrl = storedWs;
    }
    $('ws').value = state.wsUrl;
    $('ws').addEventListener('change', function (e) {
      state.wsUrl = String(e.target.value || '').trim() || P.DEFAULT_WS;
      e.target.value = state.wsUrl;
      store('ws', state.wsUrl);
      state.wsRetry = 0;
      connectWs();
      P.toast('WS 地址已更新');
    });
    $('ws-reconnect').addEventListener('click', function () {
      state.wsRetry = 0;
      connectWs();
    });

    $('identity-select').addEventListener('change', function (e) {
      if (e.target.value === '__custom__') {
        $('identity-custom').classList.remove('hidden');
        return;
      }
      $('identity-custom').classList.add('hidden');
      var m = null;
      for (var i = 0; i < state.members.length; i++) {
        if (state.members[i].user_id === e.target.value) { m = state.members[i]; break; }
      }
      if (m) setIdentity(m);
    });
    $('identity-apply').addEventListener('click', setCustomIdentity);
    $('offset').addEventListener('input', updateOffsetHint);
    $('send').addEventListener('click', send);
    $('reset').addEventListener('click', reset);
    $('text').addEventListener('keydown', function (e) {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); send(); }
    });

    document.addEventListener('visibilitychange', function () {
      if (document.hidden) {
        if (state.timer) { window.clearTimeout(state.timer); state.timer = null; }
      } else {
        pollOnce();
      }
    });

    updateOffsetHint();
    resolveWsTarget().then(connectWs);
    loadConfig().then(loadMembers).then(function () {
      if (state.members.length) {
        $('identity-select').value = state.members[0].user_id;
        setIdentity(state.members[0]);
      }
      pollOnce();
    });
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
