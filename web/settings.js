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

    return cfg;
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

  function importSnapshot(apply) {
    var path = val('snap-path').trim();
    if (!path) { banner('请填写要导入的 path', true); return; }
    var preflight=state.importPreflight;
    if (apply && (!preflight || preflight.path!==path)) { banner('请先预检当前路径的备份。',true);return; }
    var body = { path: path, apply: !!apply,revision:apply?preflight.config_revision:state.revision };
    if(apply){body.round_id=preflight.target_round_id;body.preflight_token=preflight.preflight_token;}
    $('snap-import').disabled = true;
    $('snap-apply').disabled=true;
    $('snap-import-result').textContent = apply?'正在应用…':'正在预检，不改数据…';
    P.post('/api/snapshot/import', body).then(function (res) {
      var applied = !!(res && res.applied);
      if(!applied && val('snap-path').trim()!==path) { state.importPreflight=null;$('snap-import-result').textContent='路径已变化，请重新预检。';return; }
      state.importPreflight=applied?null:Object.assign({path:path},res);
      $('snap-apply').disabled=applied||!res.applicable;
      $('snap-import-result').textContent = applied?'已应用并重算；原完整日志备份：'+res.backup_path:
        '备份轮次：'+res.manifest.round_id+' → 当前轮：'+res.target_round_id+'；消息 '+res.current_messages+' → '+res.import_messages+'。'+res.effect+(res.applicable?'':' 请先手动激活匹配轮次，再预检。');
      banner('');
      P.toast(applied?'备份已应用':'预检完成，尚未应用', 'ok');
      if (applied && window.PAIGU_SHELL) window.PAIGU_SHELL.refresh();
    }).catch(function (err) {
      if (err.status === 409) banner('导入冲突（HTTP 409）：revision 已变化，请先「重新载入」配置。', true);
      else banner('快照导入失败：' + P.errorText(err), true);
      $('snap-import-result').textContent = '导入失败：' + P.errorText(err);
    }).then(function () { $('snap-import').disabled = false; });
  }

  /* ------------------------------ 绑定 ------------------------------ */

  function init() {
    $('api').value = P.resolveApiBase();
    $('api').addEventListener('change', function (e) {
      var v = P.stripSlash(e.target.value) || P.DEFAULT_API;
      e.target.value = v;
      try { window.localStorage.setItem('paigu.display.api', v); } catch (err) { /* ignore */ }
      load().catch(function () {});
    });
    $('load').addEventListener('click', function () { load().catch(function () {}); });
    $('reload').addEventListener('click', reloadDisk);
    $('save').addEventListener('click', save);
    $('gw-reply-toggle').addEventListener('click', function () { toggleGateway('reply'); });
    $('gw-admin-toggle').addEventListener('click', function () { toggleGateway('admin'); });
    $('snap-export').addEventListener('click', exportSnapshot);
    $('snap-import').addEventListener('click', function(){importSnapshot(false);});
    $('snap-apply').addEventListener('click', function(){importSnapshot(true);});
    $('snap-path').addEventListener('input',function(){state.importPreflight=null;$('snap-apply').disabled=true;});

    load().catch(function () {});
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
