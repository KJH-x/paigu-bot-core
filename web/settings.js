(function () {
  'use strict';

  var P = window.PAIGU;
  var M = window.PAIGU_MANAGE;
  function $(id) { return document.getElementById(id); }

  var state = {
    config: null,
    revision: null,
    members: []
  };

  function banner(msg, bad) {
    var b = $('banner');
    if (!msg) { b.className = 'banner'; b.textContent = ''; return; }
    b.textContent = msg;
    b.className = 'banner show' + (bad ? ' bad' : '');
  }
  function setConn(kind, text) {
    var el = $('conn-badge');
    el.className = 'status-badge ' + (kind || '');
    el.innerHTML = '<span class="dot"></span>' + P.esc(text);
  }
  function setRev() { $('rev-badge').textContent = 'revision ' + (state.revision == null ? '—' : state.revision); }
  function val(id) { var el = $(id); return el ? el.value : ''; }
  function checked(id) { var el = $(id); return !!(el && el.checked); }
  function setVal(id, v) { var el = $(id); if (el) el.value = v == null ? '' : v; }
  function setChecked(id, v) { var el = $(id); if (el) el.checked = !!v; }

  /* ------------------------------ 表单 -> 配置 ------------------------------ */

  function applyToForm(cfg) {
    var gw = cfg.gateway || {};
    setVal('gw-bind', gw.bind);
    setVal('gw-heartbeat', gw.heartbeat_secs);
    setChecked('gw-reply', gw.reply_enabled);
    setChecked('gw-admincmds', gw.admin_commands_enabled);
    setVal('gw-whitelist', M.joinList(gw.whitelist_groups));
    setVal('gw-whitelist-members', M.joinList(gw.whitelist_members));
    setVal('gw-actions', M.joinList(gw.allowed_actions));

    var llm = cfg.llm || {};
    setChecked('llm-enabled', llm.enabled);
    setChecked('llm-fallback', llm.fallback_to_rules);
    setVal('llm-base', llm.base_url);
    setVal('llm-model', llm.model);
    setVal('llm-keyenv', llm.api_key_env);
    setVal('llm-timeout', llm.timeout_secs);
    setVal('llm-maxtokens', llm.max_tokens);
    setVal('llm-temp', llm.temperature == null ? '' : Number(Number(llm.temperature).toFixed(2)));
    setVal('llm-prompt', llm.prompt_template);

    var dp = cfg.display || {};
    setVal('dp-refresh', dp.refresh_ms);
    setVal('dp-source', dp.data_source || 'local');
    setVal('dp-remote', dp.remote_base_url);

    var mb = cfg.members || {};
    setVal('mb-group', mb.group_id);
    setVal('mb-cache', mb.cache_path);
    setVal('mb-pull', mb.daily_pull_at);

    setRev();
    $('form').classList.remove('hidden');
    $('loading').classList.add('hidden');
  }

  function collectConfig() {
    var cfg = P.deepClone(state.config) || {};
    cfg.gateway = cfg.gateway || {};
    cfg.gateway.bind = val('gw-bind');
    cfg.gateway.heartbeat_secs = P.num(val('gw-heartbeat'), cfg.gateway.heartbeat_secs);
    cfg.gateway.reply_enabled = checked('gw-reply');
    cfg.gateway.admin_commands_enabled = checked('gw-admincmds');
    cfg.gateway.whitelist_groups = M.splitList(val('gw-whitelist'));
    cfg.gateway.whitelist_members = M.splitList(val('gw-whitelist-members'));
    cfg.gateway.allowed_actions = M.splitList(val('gw-actions'));

    cfg.llm = cfg.llm || {};
    cfg.llm.enabled = checked('llm-enabled');
    cfg.llm.fallback_to_rules = checked('llm-fallback');
    cfg.llm.base_url = val('llm-base');
    cfg.llm.model = val('llm-model');
    cfg.llm.api_key_env = val('llm-keyenv');
    cfg.llm.timeout_secs = P.num(val('llm-timeout'), cfg.llm.timeout_secs);
    cfg.llm.max_tokens = P.num(val('llm-maxtokens'), cfg.llm.max_tokens);
    var temp = parseFloat(val('llm-temp'));
    cfg.llm.temperature = isNaN(temp) ? cfg.llm.temperature : temp;
    cfg.llm.prompt_template = val('llm-prompt');

    cfg.display = cfg.display || {};
    cfg.display.refresh_ms = P.num(val('dp-refresh'), cfg.display.refresh_ms);
    cfg.display.data_source = val('dp-source') || 'local';
    cfg.display.remote_base_url = val('dp-remote');

    cfg.members = cfg.members || {};
    cfg.members.group_id = val('mb-group');
    cfg.members.cache_path = val('mb-cache');
    cfg.members.daily_pull_at = val('mb-pull');
    cfg.members.cn_overrides = collectOverrides();

    return cfg;
  }

  function collectOverrides() {
    var out = [];
    (state.members || []).forEach(function (m) {
      var aliases = M.dedupe(m.aliases || []);
      // 未编辑的 CN 由前端自动填充（=清洗后群名片），不落库；仅持久化用户编辑过的 CN / 别名。
      if (!m.cnTouched && !aliases.length) return;
      out.push({ user_id: m.user_id, cn: (m.cn || '').trim(), aliases: aliases });
    });
    return out;
  }

  function load() {
    setConn('', '载入中…');
    return P.get('/api/config').then(function (res) {
      var cfg = res && res.config ? res.config : res;
      state.config = cfg;
      state.revision = res && res.revision != null ? res.revision : (cfg ? cfg.revision : null);
      applyToForm(cfg || {});
      banner('');
      setConn('ok', '已连接');
    }).catch(function (err) {
      banner('无法载入配置：' + P.errorText(err) + '。请确认本地服务已在 ' + P.resolveApiBase() + ' 运行。', true);
      setConn('bad', '未连接');
      $('loading').textContent = '配置载入失败（API 未就绪）';
      throw err;
    });
  }

  function save() {
    if (!state.config) { banner('尚未载入配置，无法保存。', true); return; }
    var cfg = collectConfig();
    $('save').disabled = true;
    P.put('/api/config', { config: cfg, revision: state.revision }).then(function (res) {
      state.config = res && res.config ? res.config : cfg;
      state.revision = res && res.revision != null ? res.revision : state.revision;
      applyToForm(state.config);
      banner('');
      P.toast('配置已保存，revision ' + state.revision, 'ok');
    }).catch(function (err) {
      if (err.status === 409) {
        banner('配置冲突（HTTP 409）：服务器上的 revision 已变化，你的改动未保存。请点「重新载入」获取最新配置后重编辑。', true);
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

  function toggleGateway(kind) {
    var key = kind === 'reply' ? 'gw-reply' : 'gw-admincmds';
    var path = kind === 'reply' ? '/api/config/reply' : '/api/config/admin-commands';
    var enabled = !checked(key);
    var body = { enabled: enabled };
    if (state.revision != null) body.revision = state.revision;
    P.post(path, body).then(function (res) {
      if (res && res.revision != null) state.revision = res.revision;
      setChecked(key, res && res.enabled != null ? res.enabled : enabled);
      setRev();
      if (state.config && state.config.gateway) {
        if (kind === 'reply') state.config.gateway.reply_enabled = !!(res && res.reply_enabled);
        else state.config.gateway.admin_commands_enabled = !!(res && res.admin_commands_enabled);
      }
      P.toast((kind === 'reply' ? 'reply_enabled' : 'admin_commands_enabled') + ' = ' + (res && res.enabled), 'ok');
    }).catch(function (err) {
      if (err.status === 409) banner('热切换冲突（HTTP 409）：revision 已变化，请先「重新载入」配置。', true);
      else banner('热切换失败：' + P.errorText(err), true);
    });
  }

  /* ------------------------------ 成员 CN ------------------------------ */

  function buildMembers(res) {
    var raw = (res && res.members) || [];
    var overrides = (state.config && state.config.members && state.config.members.cn_overrides) || [];
    var byId = {};
    overrides.forEach(function (o) { byId[o.user_id] = o; });
    return raw.map(function (m) {
      m = m || {};
      var uid = m.user_id != null ? String(m.user_id) : '';
      var ov = byId[uid];
      var card = m.card != null ? String(m.card) : '';
      var rawName = card.trim() ? card : (m.nickname != null ? String(m.nickname) : '');
      var cleaned = P.cleanNickname(rawName);
      return {
        user_id: uid,
        nickname: m.nickname != null ? String(m.nickname) : '',
        raw: rawName,
        cleaned: cleaned,
        role: m.role || '',
        resolved: m.resolved != null ? String(m.resolved) : '',
        cn: ov ? ov.cn : cleaned,
        cnTouched: !!(ov && ov.cn),
        aliases: ov ? M.asStrArray(ov.aliases) : [],
        fallback: !!m.fallback
      };
    });
  }

  function pinyinText(text) {
    var out = [];
    if (window.pinyinPro) {
      try { out = window.pinyinPro.pinyin(String(text || ''), { toneType: 'none', type: 'array' }) || []; }
      catch (e) { out = []; }
    }
    return out.join('');
  }

  function pinyinInitials(text) {
    if (!window.pinyinPro) return '';
    try {
      return window.pinyinPro.pinyin(String(text || ''), { pattern: 'first', toneType: 'none', type: 'array' }).join('');
    } catch (e) { return ''; }
  }

  function memberHaystack(m) {
    var base = [m.user_id, m.raw, m.cleaned, m.cn, m.resolved, (m.aliases || []).join(' ')].join(' ');
    return (base + ' ' + pinyinText(base) + ' ' + pinyinInitials(base)).toLowerCase();
  }

  function renderMembers() {
    var tbody = $('members-body');
    if (!tbody) return;
    while (tbody.firstChild) tbody.removeChild(tbody.firstChild);
    var filterEl = $('members-filter');
    var q = filterEl ? filterEl.value.trim().toLowerCase() : '';
    var list = state.members || [];
    var shown = 0;
    list.forEach(function (m, i) {
      if (q && memberHaystack(m).indexOf(q) < 0) return;
      shown++;
      var tr = document.createElement('tr');
      tr.innerHTML =
        '<td class="mono mono-id">' + P.esc(m.user_id) + '</td>' +
        '<td>' + P.esc(m.raw || '') + '</td>' +
        '<td><input class="clean" data-user="' + P.esc(m.user_id) + '" value="' + P.esc(m.cleaned || '') + '" /></td>' +
        '<td><input class="cn" data-user="' + P.esc(m.user_id) + '" value="' + P.esc(m.cn || '') + '" placeholder="' + P.esc(m.resolved || m.cleaned || '') + '" /></td>' +
        '<td><input class="aliases" data-user="' + P.esc(m.user_id) + '" value="' + P.esc((m.aliases || []).join(', ')) + '" placeholder="别名，逗号分隔" /></td>' +
        '<td>' + P.esc(m.role || '') + '</td>' +
        '<td><button type="button" class="small" data-use="' + i + '">用当前昵称</button></td>';
      tbody.appendChild(tr);
    });
    $('members-empty').hidden = shown > 0;
    $('members-info').textContent = (q ? shown + ' / ' : '') + list.length + ' 人' + (list.length && list[0].fallback ? '（内置子集，未接入群）' : '');
  }

  function onMembersInput(ev) {
    var t = ev.target;
    if (!t || !t.dataset || !t.dataset.user) return;
    var uid = t.dataset.user;
    var entry = null;
    for (var i = 0; i < state.members.length; i++) {
      if (state.members[i].user_id === uid) { entry = state.members[i]; break; }
    }
    if (!entry) return;
    if (t.classList.contains('clean')) {
      entry.cleaned = t.value;
      if (!entry.cnTouched) entry.cn = t.value;
      entry.cnTouched = true;
      return;
    }
    if (t.classList.contains('aliases')) entry.aliases = M.splitTokens(t.value);
    else { entry.cn = t.value; entry.cnTouched = true; }
  }

  function onMembersClick(ev) {
    var b = ev.target.closest ? ev.target.closest('button[data-use]') : null;
    if (!b) return;
    var i = parseInt(b.getAttribute('data-use'), 10);
    var m = state.members[i];
    if (!m) return;
    m.cn = m.cleaned || m.resolved || m.nickname || '';
    m.cnTouched = true;
    renderMembers();
  }

  function loadMembers() {
    return P.get('/api/members').then(function (res) {
      state.members = buildMembers(res && res.members !== undefined ? res : { members: res });
      if (!state.members.length) state.members = P.embeddedMembers();
      renderMembers();
    }).catch(function (err) {
      state.members = P.embeddedMembers();
      renderMembers();
      banner('成员列表加载失败：' + P.errorText(err) + '。已显示内置子集（DESIGN §8）。', false);
    });
  }

  function refreshMembers() {
    $('members-refresh').disabled = true;
    $('members-info').textContent = '拉取中…';
    P.post('/api/members/refresh', {}).then(function () { return loadMembers(); })
      .then(function () { P.toast('成员列表已刷新', 'ok'); })
      .catch(function (err) {
        banner('拉取群成员失败：' + P.errorText(err) + '（需 NapCat 已接入且 Gateway 在线）。', true);
        state.members = P.embeddedMembers();
        renderMembers();
      }).then(function () { $('members-refresh').disabled = false; });
  }

  function llmCleanMembers() {
    var btn = $('members-cn-llm');
    if (btn) btn.disabled = true;
    P.post('/api/members/cn-suggest', {}).then(function (res) {
      var sug = (res && res.suggestions) || [];
      var byId = {};
      sug.forEach(function (s) { byId[s.user_id] = s.cn; });
      var filled = 0;
      (state.members || []).forEach(function (m) {
        if (byId[m.user_id]) { m.cn = byId[m.user_id]; m.cnTouched = true; filled++; }
      });
      renderMembers();
      banner('LLM 清理完成：' + filled + ' 条建议已填入 CN，可编辑后保存。', true);
    }).catch(function (err) {
      banner('LLM 清理失败：' + P.errorText(err), false);
    }).then(function () { if (btn) btn.disabled = false; });
  }

  /* ------------------------------ 快照 ------------------------------ */
  function exportSnapshot() {
    var out = val('snap-out').trim();
    var body = { format: 'file' };
    if (out) body.out_dir = out;
    $('snap-export').disabled = true;
    $('snap-export-info').textContent = '导出中…';
    P.post('/api/snapshot/export', body).then(function (res) {
      var path = res && res.path ? res.path : '(未返回路径)';
      var extra = '';
      if (res && res.manifest) {
        extra = '（manifest: round_id=' + (res.manifest.round_id || '?') + '，revision=' + (res.manifest.revision != null ? res.manifest.revision : '?') + '）';
      }
      $('snap-export-info').textContent = '已导出：' + path + ' ' + extra;
      if (res && res.path) setVal('snap-path', res.path);
      banner('');
      P.toast('快照已导出', 'ok');
    }).catch(function (err) {
      $('snap-export-info').textContent = '导出失败：' + P.errorText(err);
      banner('快照导出失败：' + P.errorText(err), true);
    }).then(function () { $('snap-export').disabled = false; });
  }

  function importSnapshot() {
    var path = val('snap-path').trim();
    if (!path) { banner('请填写要导入的 path', true); return; }
    var body = { path: path, apply: checked('snap-apply') };
    if (state.revision != null) body.revision = state.revision;
    $('snap-import').disabled = true;
    $('snap-import-result').textContent = '导入中…';
    P.post('/api/snapshot/import', body).then(function (res) {
      var applied = !!(res && res.applied);
      $('snap-import-result').textContent = '导入成功：verified=' + !!(res && res.verified) + '，applied=' + applied +
        (res && res.manifest ? '，round_id=' + (res.manifest.round_id || '?') : '');
      banner('');
      P.toast('快照导入成功', 'ok');
      if (applied) msg.load();
    }).catch(function (err) {
      if (err.status === 409) banner('导入冲突（HTTP 409）：revision 已变化，请先「重新载入」配置。', true);
      else banner('快照导入失败：' + P.errorText(err), true);
      $('snap-import-result').textContent = '导入失败：' + P.errorText(err);
    }).then(function () { $('snap-import').disabled = false; });
  }

  /* ------------------------------ 消息日志 ------------------------------ */

  var msg = window.PAIGU_MSGLOG.create({
    tbody: 'msg-body',
    empty: 'msg-empty',
    info: 'msg-info',
    refresh: 'msg-refresh',
    getRevision: function () { return state.revision; },
    setRevision: function (v) { state.revision = v; setRev(); },
    onBanner: banner,
    onToast: function (t, k) { P.toast(t, k); }
  });

  /* ------------------------------ 绑定 ------------------------------ */

  function init() {
    $('api').value = P.resolveApiBase();
    $('api').addEventListener('change', function (e) {
      var v = P.stripSlash(e.target.value) || P.DEFAULT_API;
      e.target.value = v;
      try { window.localStorage.setItem('paigu.display.api', v); } catch (err) { /* ignore */ }
      load().then(loadMembers).catch(function () {});
    });
    $('load').addEventListener('click', function () { load().then(loadMembers).catch(function () {}); });
    $('reload').addEventListener('click', reloadDisk);
    $('save').addEventListener('click', save);
    $('gw-reply-toggle').addEventListener('click', function () { toggleGateway('reply'); });
    $('gw-admin-toggle').addEventListener('click', function () { toggleGateway('admin'); });
    $('members-refresh').addEventListener('click', refreshMembers);
  if ($('members-cn-llm')) $('members-cn-llm').addEventListener('click', llmCleanMembers);
    $('members-cn-save').addEventListener('click', save);
  $('members-body').addEventListener('input', onMembersInput);
  $('members-body').addEventListener('click', onMembersClick);
  if ($('members-filter')) $('members-filter').addEventListener('input', renderMembers);
    $('snap-export').addEventListener('click', exportSnapshot);
    $('snap-import').addEventListener('click', importSnapshot);

    load().then(loadMembers).catch(function () {});
    msg.load();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
