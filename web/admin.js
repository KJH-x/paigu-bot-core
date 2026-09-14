(function () {
  'use strict';

  var P = window.PAIGU;

  function $(id) { return document.getElementById(id); }

  var state = {
    config: null,
    revision: null,
    items: [],
    apiBase: P.resolveApiBase()
  };

  function store(key, val) {
    try { window.localStorage.setItem('paigu.admin.' + key, val); } catch (e) { /* ignore */ }
  }
  function readStore(key) {
    try { return window.localStorage.getItem('paigu.admin.' + key); } catch (e) { return null; }
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
    el.innerHTML = '<span class="dot"></span>' + P.esc(text);
  }

  function val(id) { var el = $(id); return el ? el.value : ''; }
  function checked(id) { var el = $(id); return !!(el && el.checked); }
  function setVal(id, v) { var el = $(id); if (el) el.value = v == null ? '' : v; }
  function setChecked(id, v) { var el = $(id); if (el) el.checked = !!v; }

  function splitList(s) {
    return String(s == null ? '' : s).split(/[,，\n]/).map(function (x) { return x.trim(); }).filter(function (x) { return x !== ''; });
  }
  function joinList(a) { return P.asArray(a).join(', '); }

  function pad2(n) { return (n < 10 ? '0' : '') + n; }
  function msToLocalInput(ms) {
    if (ms == null) return '';
    var d = new Date(ms);
    if (isNaN(d.getTime())) return '';
    return d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate()) + 'T' + pad2(d.getHours()) + ':' + pad2(d.getMinutes());
  }
  function localInputToMs(s) {
    if (!s) return null;
    var d = new Date(s);
    return isNaN(d.getTime()) ? null : d.getTime();
  }

  function updateWindowMs() {
    var s = localInputToMs(val('rd-start'));
    var e = localInputToMs(val('rd-end'));
    $('rd-window-ms').textContent = 'start_ms=' + (s == null ? '—' : s) + ' · end_ms=' + (e == null ? '—' : e);
  }

  function applyToForm(cfg) {
    var gw = cfg.gateway || {};
    setVal('gw-bind', gw.bind);
    setVal('gw-heartbeat', gw.heartbeat_secs);
    setChecked('gw-reply', gw.reply_enabled);
    setVal('gw-whitelist', joinList(gw.whitelist_groups));
    setVal('gw-actions', joinList(gw.allowed_actions));

    var llm = cfg.llm || {};
    setChecked('llm-enabled', llm.enabled);
    setChecked('llm-fallback', llm.fallback_to_rules);
    setVal('llm-base', llm.base_url);
    setVal('llm-model', llm.model);
    setVal('llm-keyenv', llm.api_key_env);
    setVal('llm-timeout', llm.timeout_secs);
    setVal('llm-maxtokens', llm.max_tokens);
    setVal('llm-temp', llm.temperature);
    setVal('llm-prompt', llm.prompt_template);

    var rd = cfg.round || {};
    setVal('rd-id', rd.round_id);
    setVal('rd-title', rd.title);
    setVal('rd-group', rd.group_id);
    setVal('rd-priority', joinList(rd.priority_users));
    var win = rd.priority_window || {};
    setVal('rd-start', msToLocalInput(win.start_ms));
    setVal('rd-end', msToLocalInput(win.end_ms));
    updateWindowMs();

    var dp = cfg.display || {};
    setVal('dp-refresh', dp.refresh_ms);
    setVal('dp-source', dp.data_source || 'local');
    setVal('dp-remote', dp.remote_base_url);

    var mb = cfg.members || {};
    setVal('mb-group', mb.group_id);
    setVal('mb-cache', mb.cache_path);
    setVal('mb-pull', mb.daily_pull_at);

    state.items = P.deepClone(P.asArray(rd.items));
    renderItems();

    $('rev-badge').textContent = 'revision ' + (state.revision == null ? '—' : state.revision);
    $('form').classList.remove('hidden');
    $('loading').classList.add('hidden');
  }

  function inputAttr(scope, i, vi, f) {
    var s = ' data-scope="' + scope + '" data-i="' + i + '" data-f="' + f + '"';
    if (vi != null) s += ' data-vi="' + vi + '"';
    return s;
  }

  function itemCard(item, i) {
    var card = document.createElement('div');
    card.className = 'card';
    var head = document.createElement('div');
    head.className = 'card-head';
    head.innerHTML = '<span>商品 #' + (i + 1) + '</span><span class="spacer"></span>';
    var del = document.createElement('button');
    del.className = 'danger small';
    del.textContent = '删除商品';
    del.addEventListener('click', function () {
      state.items.splice(i, 1);
      renderItems();
    });
    head.appendChild(del);
    card.appendChild(head);

    var grid = document.createElement('div');
    grid.className = 'split';
    grid.innerHTML =
      '<label class="field"><span>item_id</span><input' + inputAttr('item', i, null, 'item_id') + ' value="' + P.esc(item.item_id) + '" /></label>' +
      '<label class="field"><span>name</span><input' + inputAttr('item', i, null, 'name') + ' value="' + P.esc(item.name) + '" /></label>' +
      '<label class="field"><span>kind</span><select' + inputAttr('item', i, null, 'kind') + '>' +
        ['split', 'single', 'gift'].map(function (k) {
          return '<option value="' + k + '"' + (item.kind === k ? ' selected' : '') + '>' + k + '</option>';
        }).join('') + '</select></label>' +
      '<label class="field"><span>aliases（逗号分隔）</span><input' + inputAttr('item', i, null, 'aliases') + ' value="' + P.esc(joinList(item.aliases)) + '" /></label>';
    card.appendChild(grid);

    var vhead = document.createElement('div');
    vhead.className = 'row';
    vhead.style.margin = '6px 0';
    vhead.innerHTML = '<strong>变体</strong><span class="spacer" style="flex:1"></span>';
    var addV = document.createElement('button');
    addV.className = 'small';
    addV.textContent = '添加变体';
    addV.addEventListener('click', function () {
      if (!Array.isArray(state.items[i].variants)) state.items[i].variants = [];
      state.items[i].variants.push({ variant_id: 'v_' + Date.now(), name: '', capacity: null, aliases: [] });
      renderItems();
    });
    vhead.appendChild(addV);
    card.appendChild(vhead);

    var vhost = document.createElement('div');
    var variants = P.asArray(item.variants);
    for (var vi = 0; vi < variants.length; vi++) {
      vhost.appendChild(variantRow(i, vi, variants[vi]));
    }
    if (!variants.length) {
      var none = document.createElement('div');
      none.className = 'hint';
      none.textContent = '无变体（单领/整体商品）';
      vhost.appendChild(none);
    }
    card.appendChild(vhost);
    return card;
  }

  function variantRow(i, vi, v) {
    var row = document.createElement('div');
    row.className = 'row';
    row.style.marginBottom = '6px';
    row.innerHTML =
      '<input placeholder="variant_id"' + inputAttr('variant', i, vi, 'variant_id') + ' value="' + P.esc(v.variant_id) + '" style="flex:1" />' +
      '<input placeholder="name"' + inputAttr('variant', i, vi, 'name') + ' value="' + P.esc(v.name) + '" style="flex:1" />' +
      '<input placeholder="capacity"' + inputAttr('variant', i, vi, 'capacity') + ' value="' + P.esc(v.capacity == null ? '' : v.capacity) + '" type="number" min="0" style="width:90px" />' +
      '<input placeholder="aliases"' + inputAttr('variant', i, vi, 'aliases') + ' value="' + P.esc(joinList(v.aliases)) + '" style="flex:1" />';
    var del = document.createElement('button');
    del.className = 'danger small';
    del.textContent = '×';
    del.addEventListener('click', function () {
      state.items[i].variants.splice(vi, 1);
      renderItems();
    });
    row.appendChild(del);
    return row;
  }

  function renderItems() {
    var host = $('items');
    while (host.firstChild) host.removeChild(host.firstChild);
    for (var i = 0; i < state.items.length; i++) {
      host.appendChild(itemCard(state.items[i] || {}, i));
    }
    if (!state.items.length) {
      var e = document.createElement('div');
      e.className = 'empty';
      e.textContent = '暂无商品，点击「添加商品」';
      host.appendChild(e);
    }
  }

  function onItemsInput(ev) {
    var t = ev.target;
    if (!t || !t.dataset || !t.dataset.scope) return;
    var i = parseInt(t.dataset.i, 10);
    var f = t.dataset.f;
    var item = state.items[i];
    if (!item) return;
    if (t.dataset.scope === 'item') {
      if (f === 'aliases') item.aliases = splitList(t.value);
      else item[f] = t.value;
    } else if (t.dataset.scope === 'variant') {
      var vi = parseInt(t.dataset.vi, 10);
      if (!item.variants || !item.variants[vi]) return;
      var v = item.variants[vi];
      if (f === 'aliases') v.aliases = splitList(t.value);
      else if (f === 'capacity') v.capacity = t.value === '' ? null : P.num(t.value, null);
      else v[f] = t.value;
    }
  }

  function collectFromForm() {
    var cfg = P.deepClone(state.config) || {};
    cfg.gateway = cfg.gateway || {};
    cfg.gateway.bind = val('gw-bind');
    cfg.gateway.heartbeat_secs = P.num(val('gw-heartbeat'), cfg.gateway.heartbeat_secs);
    cfg.gateway.reply_enabled = checked('gw-reply');
    cfg.gateway.whitelist_groups = splitList(val('gw-whitelist'));
    cfg.gateway.allowed_actions = splitList(val('gw-actions'));

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

    cfg.round = cfg.round || {};
    cfg.round.round_id = val('rd-id');
    cfg.round.title = val('rd-title');
    cfg.round.group_id = val('rd-group');
    cfg.round.priority_users = splitList(val('rd-priority'));
    cfg.round.priority_window = cfg.round.priority_window || {};
    var s = localInputToMs(val('rd-start'));
    var e = localInputToMs(val('rd-end'));
    if (s != null) cfg.round.priority_window.start_ms = s;
    if (e != null) cfg.round.priority_window.end_ms = e;
    cfg.round.items = state.items;

    cfg.display = cfg.display || {};
    cfg.display.refresh_ms = P.num(val('dp-refresh'), cfg.display.refresh_ms);
    cfg.display.data_source = val('dp-source') || 'local';
    cfg.display.remote_base_url = val('dp-remote');

    cfg.members = cfg.members || {};
    cfg.members.group_id = val('mb-group');
    cfg.members.cache_path = val('mb-cache');
    cfg.members.daily_pull_at = val('mb-pull');

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
      setConn('ok', '已连接');
    }).catch(function (err) {
      banner('无法载入配置：' + P.errorText(err) + '。请确认本地服务已在 ' + state.apiBase + ' 运行。', true);
      setConn('bad', '未连接');
      $('loading').textContent = '配置载入失败（API 未就绪）';
      throw err;
    });
  }

  function save() {
    if (!state.config) { banner('尚未载入配置，无法保存。', true); return; }
    var cfg = collectFromForm();
    $('save').disabled = true;
    P.put('/api/config', { config: cfg, revision: state.revision }).then(function (res) {
      state.config = res && res.config ? res.config : cfg;
      state.revision = res && res.revision != null ? res.revision : state.revision;
      applyToForm(state.config);
      hideBanner();
      P.toast('配置已保存，revision ' + state.revision, 'ok');
    }).catch(function (err) {
      if (err.status === 409) {
        banner('配置冲突（HTTP 409）：服务器上的 revision 已变化，你的改动未保存。请点击「重新载入」获取最新配置后重新编辑。', true);
      } else {
        banner('保存失败：' + P.errorText(err), true);
      }
    }).then(function () {
      $('save').disabled = false;
    });
  }

  function reloadDisk() {
    $('reload').disabled = true;
    P.post('/api/config/reload', {}).then(function () {
      return load();
    }).then(function () {
      P.toast('已从磁盘重载配置', 'ok');
    }).catch(function (err) {
      banner('从磁盘重载失败：' + P.errorText(err), true);
    }).then(function () {
      $('reload').disabled = false;
    });
  }

  function renderMembers(list) {
    var tbody = $('members-body');
    while (tbody.firstChild) tbody.removeChild(tbody.firstChild);
    for (var i = 0; i < list.length; i++) {
      var m = list[i];
      var tr = document.createElement('tr');
      var raw = m.nickname || '';
      var cleaned = m.cleaned || P.cleanNickname(raw);
      tr.innerHTML =
        '<td class="mono">' + P.esc(m.user_id) + '</td>' +
        '<td>' + P.esc(cleaned) + '</td>' +
        '<td class="hint">' + P.esc(cleaned !== raw ? raw : '') + '</td>' +
        '<td>' + P.esc(m.role || '') + '</td>';
      tbody.appendChild(tr);
    }
    $('members-empty').hidden = list.length > 0;
    $('members-info').textContent = list.length + ' 人' + (list.length && list[0].fallback ? '（内置子集，未接入群）' : '');
  }

  function loadMembers() {
    return P.get('/api/members').then(function (res) {
      var list = P.normalizeMembers(res);
      if (!list.length) list = P.embeddedMembers();
      renderMembers(list);
    }).catch(function (err) {
      renderMembers(P.embeddedMembers());
      banner('成员列表加载失败：' + P.errorText(err) + '。已显示内置子集（DESIGN §8）。', false);
    });
  }

  function refreshMembers() {
    $('members-refresh').disabled = true;
    $('members-info').textContent = '拉取中…';
    P.post('/api/members/refresh', {}).then(function () {
      return loadMembers();
    }).then(function () {
      P.toast('成员列表已刷新', 'ok');
    }).catch(function (err) {
      banner('拉取群成员失败：' + P.errorText(err) + '（需 NapCat 已接入且 Gateway 在线）。', true);
      renderMembers(P.embeddedMembers());
    }).then(function () {
      $('members-refresh').disabled = false;
    });
  }

  function init() {
    var stored = readStore('api');
    if (!P.qs('api') && stored) state.apiBase = P.stripSlash(stored);
    $('api').value = state.apiBase;

    $('api').addEventListener('change', function (e) {
      state.apiBase = P.stripSlash(e.target.value) || P.DEFAULT_API;
      e.target.value = state.apiBase;
      store('api', state.apiBase);
      P.toast('API 地址已更新');
    });
    $('load').addEventListener('click', function () { load().catch(function () {}); });
    $('reload').addEventListener('click', reloadDisk);
    $('save').addEventListener('click', save);
    $('item-add').addEventListener('click', function () {
      state.items.push({ item_id: 'item_' + Date.now(), name: '', kind: 'split', aliases: [], variants: [] });
      renderItems();
    });
    $('items').addEventListener('input', onItemsInput);
    $('items').addEventListener('change', onItemsInput);
    $('rd-start').addEventListener('input', updateWindowMs);
    $('rd-end').addEventListener('input', updateWindowMs);
    $('members-refresh').addEventListener('click', refreshMembers);

    load().catch(function () {}).then(loadMembers);
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
