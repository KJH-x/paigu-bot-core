(function (global) {
  'use strict';

  var P = global.PAIGU;
  var M = global.PAIGU_MANAGE;

  /**
   * 共享消息日志编辑器（设置页 / 排谷界面折叠面板复用）。
   * opts = {
   *   tbody, empty, info, refresh,        // 元素 id
   *   getRevision, setRevision,           // 乐观并发
   *   onBanner(msg, bad), onToast(msg, kind)
   * }
   */
  function create(opts) {
    opts = opts || {};
    function $(id) { return document.getElementById(id); }
    var state = { messages: [] };

    function banner(msg, bad) { if (opts.onBanner) opts.onBanner(msg, bad); }
    function toast(msg, kind) { if (opts.onToast) opts.onToast(msg, kind); }

    function render() {
      var tbody = $(opts.tbody);
      if (!tbody) return;
      while (tbody.firstChild) tbody.removeChild(tbody.firstChild);
      var list = state.messages || [];
      var emptyEl = $(opts.empty);
      if (emptyEl) emptyEl.hidden = list.length > 0;
      var infoEl = $(opts.info);
      if (infoEl) infoEl.textContent = list.length ? (list.length + ' 条') : '';
      for (var i = 0; i < list.length; i++) tbody.appendChild(row(list[i]));
    }

    function row(m) {
      var tr = document.createElement('tr');

      var tdSeq = document.createElement('td');
      tdSeq.className = 'mono';
      var shortId = String(m.record_id || '').slice(0, 8);
      tdSeq.textContent = '#' + m.seq + (shortId ? ' · ' + shortId : '');
      tdSeq.title = (m.record_id || '') + ' / revision ' + (m.record_revision || 1);
      tr.appendChild(tdSeq);

      var tdText = document.createElement('td');
      var textInp = document.createElement('input');
      textInp.value = m.text == null ? '' : m.text;
      textInp.style.width = '100%';
      textInp.setAttribute('data-field', 'text');
      tdText.appendChild(textInp);
      var meta = document.createElement('div');
      meta.className = 'hint mono';
      meta.textContent = (m.nickname || m.user_id || '—') + ' · 群 ' + (m.group_id || '—');
      tdText.appendChild(meta);
      tr.appendChild(tdText);

      var tdTs = document.createElement('td');
      var tsInp = document.createElement('input');
      tsInp.type = 'datetime-local';
      tsInp.value = M.msToLocalInput(m.timestamp_ms);
      tsInp.setAttribute('data-field', 'timestamp_ms');
      tdTs.appendChild(tsInp);
      tr.appendChild(tdTs);

      var tdStatus = document.createElement('td');
      var status = document.createElement('div');
      status.textContent = m.status == null ? '' : m.status;
      var route = document.createElement('div');
      route.className = 'hint mono';
      route.textContent = m.routed || '—';
      route.title = m.detail || '';
      tdStatus.appendChild(status);
      tdStatus.appendChild(route);
      tr.appendChild(tdStatus);

      var tdAct = document.createElement('td');
      var save = document.createElement('button');
      save.className = 'primary small';
      save.textContent = '保存';
      save.addEventListener('click', function () { saveMessage(tr, m); });
      tdAct.appendChild(save);
      var reroute = document.createElement('button');
      reroute.className = 'small';
      reroute.textContent = '重新路由';
      reroute.addEventListener('click', function () { rerouteMessage(m); });
      tdAct.appendChild(reroute);
      var del = document.createElement('button');
      del.className = 'danger small';
      del.textContent = '删除';
      del.addEventListener('click', function () { deleteMessage(m); });
      tdAct.appendChild(del);
      tr.appendChild(tdAct);

      return tr;
    }

    function load() {
      var refresh = $(opts.refresh);
      if (refresh) refresh.disabled = true;
      return P.get('/api/messages?limit=500').then(function (res) {
        state.messages = (res && res.messages) || [];
        render();
      }).catch(function (err) {
        banner('消息加载失败：' + P.errorText(err), true);
      }).then(function () {
        if (refresh) refresh.disabled = false;
      });
    }

    function saveMessage(tr, m) {
      var seq = m.seq;
      var key = m.record_id || String(seq);
      var textValue = tr.querySelector('[data-field="text"]').value;
      var ts = M.localInputToMs(tr.querySelector('[data-field="timestamp_ms"]').value);
      if (!textValue.trim()) { banner('消息 text 不能为空', true); return; }
      if (ts == null) { banner('消息时间非法：请填写有效的本地时间', true); return; }
      var body = { text: textValue, timestamp_ms: ts, record_revision: m.record_revision, recompute: true };
      P.put('/api/messages/' + encodeURIComponent(key), body).then(function (res) {
        banner('');
        var s = res && res.result && res.result.summary;
        toast('消息 ' + seq + ' 已保存并重算' + (s ? '（应用 ' + s.applied + '，掉落 ' + s.dropped + '）' : ''), 'ok');
        return load();
      }).catch(function (err) {
        if (err.status === 404) banner('消息 ' + seq + ' 不存在（可能已被删除）。', true);
        else if (err.status === 409) banner('该消息已被其他操作修改，请刷新后重试。', true);
        else if (err.status === 423) banner('轮次已锁定，请先在结算页解锁。', true);
        else banner('保存消息失败：' + P.errorText(err), true);
      });
    }

    function deleteMessage(m) {
      var key = m.record_id || String(m.seq);
      if (!global.confirm('确认删除消息 seq=' + m.seq + ' 并触发重算？')) return;
      P.request('/api/messages/' + encodeURIComponent(key) + '?recompute=true&record_revision=' + encodeURIComponent(m.record_revision || 1), { method: 'DELETE' }).then(function () {
        banner('');
        toast('消息 ' + m.seq + ' 已删除并重算', 'ok');
        return load();
      }).catch(function (err) {
        if (err.status === 404) banner('消息 ' + m.seq + ' 不存在。', true);
        else if (err.status === 409) banner('该消息已变化，请刷新后重试。', true);
        else if (err.status === 423) banner('轮次已锁定，请先在结算页解锁。', true);
        else banner('删除消息失败：' + P.errorText(err), true);
      });
    }

    function rerouteMessage(m) {
      var key = m.record_id || String(m.seq);
      if (!global.confirm('按当前群与成员白名单重新判定消息 seq=' + m.seq + ' 的路由？')) return;
      P.post('/api/messages/' + encodeURIComponent(key) + '/reroute', { record_revision: m.record_revision }).then(function (res) {
        var s = res && res.result && res.result.summary;
        banner('');
        toast('已重新路由' + (s ? '（应用 ' + s.applied + '，掉落 ' + s.dropped + '）' : ''), 'ok');
        return load();
      }).catch(function (err) {
        if (err.status === 409) banner('该消息已变化，请刷新后重试。', true);
        else if (err.status === 423) banner('轮次已锁定，请先在结算页解锁。', true);
        else banner('重新路由失败：' + P.errorText(err), true);
      });
    }

    if ($(opts.refresh)) $(opts.refresh).addEventListener('click', function () { load(); });

    return { load: load, render: render, state: state };
  }

  global.PAIGU_MSGLOG = { create: create };
})(window);
