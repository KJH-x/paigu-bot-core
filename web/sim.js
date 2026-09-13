(function () {
  'use strict';

  var P = window.PAIGU;

  function $(id) { return document.getElementById(id); }

  var state = {
    apiBase: P.resolveApiBase(),
    refreshMs: P.INLINE.refreshMs || 3000,
    version: 0,
    meta: null,
    config: null,
    groupId: '',
    items: [],
    turns: [],
    turnSeq: 0,
    members: [],
    identity: null,
    timer: null,
    inFlight: false,
    failures: 0
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
    var seq = t.version != null ? 'v#' + t.version : '';
    if (el.__seq.textContent !== seq) el.__seq.textContent = seq;
    if (el.__body.textContent !== t.text) el.__body.textContent = t.text;
    var out = outcomeInfo(t.outcome);
    if (el.__out.textContent !== out.text) el.__out.textContent = out.text;
    var cls = 'out' + (out.cls ? ' ' + out.cls : '');
    if (el.__out.className !== cls) el.__out.className = cls;
  }

  function renderTurns() {
    P.syncKeyedChildren($('transcript'), state.turns, function (t) { return t.key; }, createTurn, updateTurn);
    $('turn-count').textContent = state.turns.length ? state.turns.length + ' 条' : '';
    $('transcript-empty').hidden = state.turns.length > 0;
  }

  function pushTurn(text, outcome, version, offsetText) {
    state.turnSeq++;
    state.turns.push({
      key: 't' + state.turnSeq,
      display: state.identity ? (state.identity.nickname || state.identity.user_id) : '?',
      text: text,
      offsetText: offsetText,
      outcome: outcome,
      version: version
    });
    renderTurns();
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

  function buildIdentitySelect() {
    var sel = $('identity-select');
    while (sel.firstChild) sel.removeChild(sel.firstChild);
    for (var i = 0; i < state.members.length; i++) {
      var m = state.members[i];
      var opt = document.createElement('option');
      opt.value = m.user_id;
      opt.textContent = m.cleaned + (m.fallback ? '（内置子集）' : '') + (P.PRIORITY_USERS.indexOf(m.cleaned) >= 0 ? ' · 预存' : '');
      sel.appendChild(opt);
    }
    var custom = document.createElement('option');
    custom.value = '__custom__';
    custom.textContent = '＋ 新建/自定义身份';
    sel.appendChild(custom);
  }

  function setIdentity(member) {
    var isPriority = P.PRIORITY_USERS.indexOf(P.cleanNickname(member.nickname || member.cleaned || '')) >= 0;
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
    state.identity = {
      user_id: userId,
      nickname: nick || userId,
      is_admin: $('c-admin').checked,
      priority: $('c-priority').checked,
      priority_level: $('c-priority').checked ? (P.num($('c-level').value, 10) || 10) : 0
    };
    $('identity-info').textContent = state.identity.nickname + ' · ' + state.identity.user_id + (state.identity.priority ? ' · 预存' : '');
    postIdentity();
    P.toast('已切换为自定义身份 ' + state.identity.nickname);
  }

  function postIdentity() {
    if (!state.identity) return;
    P.post('/api/sim/identity', {
      user_id: state.identity.user_id,
      nickname: state.identity.nickname,
      is_admin: state.identity.is_admin,
      priority: state.identity.priority,
      priority_level: state.identity.priority_level
    }).catch(function (err) {
      P.toast('身份设置失败：' + P.errorText(err), 'bad');
    });
  }

  function send() {
    if (!state.identity) { P.toast('请先选择身份', 'bad'); return; }
    var text = $('text').value.trim();
    if (!text) { P.toast('请输入消息内容', 'bad'); return; }
    var ms = currentOffsetMs();
    if (ms === null) { P.toast('时间偏移格式无效', 'bad'); return; }

    var body = {
      user_id: state.identity.user_id,
      nickname: state.identity.nickname,
      text: text,
      offset_ms: ms,
      is_admin: state.identity.is_admin
    };
    var g = $('group').value.trim();
    if (g) body.group_id = g;

    $('send').disabled = true;
    $('send-hint').textContent = '发送中…';
    P.post('/api/sim/message', body).then(function (resp) {
      $('send-hint').textContent = '';
      pushTurn(text, resp && resp.outcome, resp && resp.version, P.formatOffset(ms));
      applyBoardPayload(resp);
      if (resp && resp.version != null) setVersion(resp.version);
      $('text').value = '';
      hideBanner();
    }).catch(function (err) {
      $('send-hint').textContent = '';
      showBanner('发送失败：' + P.errorText(err), true);
      pushTurn(text, { status: 'error', detail: P.errorText(err) }, null, P.formatOffset(ms));
    }).then(function () {
      $('send').disabled = false;
    });
  }

  function reset() {
    $('reset').disabled = true;
    P.post('/api/sim/reset', {}).then(function () {
      state.turns = [];
      state.turnSeq = 0;
      state.items = [];
      state.version = 0;
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
    P.get('/api/display?since=' + encodeURIComponent(state.version)).then(function (payload) {
      if (payload && payload.board !== undefined) applyBoardPayload(payload);
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
      var list = P.normalizeMembers(res);
      state.members = list.length ? list : P.embeddedMembers();
      if (!list.length) showBanner('未能从 API 获取成员，已使用内置子集（POLICY §8）。', false);
      buildIdentitySelect();
    }).catch(function () {
      state.members = P.embeddedMembers();
      buildIdentitySelect();
      showBanner('成员列表不可用，已使用内置子集（POLICY §8）。', false);
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
