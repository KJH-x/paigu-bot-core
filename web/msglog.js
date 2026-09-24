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

    function rev() { return typeof opts.getRevision === 'function' ? opts.getRevision() : null; }
    function setRev(v) { if (typeof opts.setRevision === 'function') opts.setRevision(v); }
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
      tdSeq.textContent = m.seq;
      tr.appendChild(tdSeq);

      var tdText = document.createElement('td');
      var textInp = document.createElement('input');
      textInp.value = m.text == null ? '' : m.text;
      textInp.style.width = '100%';
      textInp.setAttribute('data-field', 'text');
      tdText.appendChild(textInp);
      tr.appendChild(tdText);

      var tdTs = document.createElement('td');
      var tsInp = document.createElement('input');
      tsInp.type = 'datetime-local';
      tsInp.value = M.msToLocalInput(m.timestamp_ms);
      tsInp.setAttribute('data-field', 'timestamp_ms');
      tdTs.appendChild(tsInp);
      tr.appendChild(tdTs);

      var tdStatus = document.createElement('td');
      var statusInp = document.createElement('input');
      statusInp.value = m.status == null ? '' : m.status;
      statusInp.setAttribute('data-field', 'status');
      tdStatus.appendChild(statusInp);
      tr.appendChild(tdStatus);

      var tdAct = document.createElement('td');
      var save = document.createElement('button');
      save.className = 'primary small';
      save.textContent = '保存';
      save.addEventListener('click', function () { saveMessage(tr, m); });
      tdAct.appendChild(save);
      var del = document.createElement('button');
      del.className = 'danger small';
      del.textContent = '删除';
      del.addEventListener('click', function () { deleteMessage(m.seq); });
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
      var textValue = tr.querySelector('[data-field="text"]').value;
      var statusValue = tr.querySelector('[data-field="status"]').value;
      var ts = M.localInputToMs(tr.querySelector('[data-field="timestamp_ms"]').value);
      if (!textValue.trim()) { banner('消息 text 不能为空', true); return; }
      if (ts == null) { banner('消息时间非法：请填写有效的本地时间', true); return; }
      var body = { text: textValue, status: statusValue, timestamp_ms: ts, recompute: true };
      // 同步不可变字段，避免后端内存记录缺 user_id/group_id 时校验失败
      if (m.user_id) body.user_id = m.user_id;
      if (m.nickname) body.nickname = m.nickname;
      if (m.group_id) body.group_id = m.group_id;
      if (m.routed) body.routed = m.routed;
      if (typeof m.is_admin === 'boolean') body.is_admin = m.is_admin;
      var r = rev();
      if (r != null) body.revision = r;
      P.put('/api/messages/' + encodeURIComponent(seq), body).then(function (res) {
        if (res && res.revision != null) setRev(res.revision);
        banner('');
        toast('消息 ' + seq + ' 已保存并重算', 'ok');
        return load();
      }).catch(function (err) {
        if (err.status === 404) banner('消息 ' + seq + ' 不存在（可能已被删除）。', true);
        else if (err.status === 409) banner('保存冲突（HTTP 409）：revision 已变化，请先「重新载入」配置。', true);
        else banner('保存消息失败：' + P.errorText(err), true);
      });
    }

    function deleteMessage(seq) {
      if (!global.confirm('确认删除消息 seq=' + seq + ' 并触发重算？')) return;
      P.request('/api/messages/' + encodeURIComponent(seq) + '?recompute=true', { method: 'DELETE' }).then(function (res) {
        if (res && res.revision != null) setRev(res.revision);
        banner('');
        toast('消息 ' + seq + ' 已删除并重算', 'ok');
        return load();
      }).catch(function (err) {
        if (err.status === 404) banner('消息 ' + seq + ' 不存在。', true);
        else banner('删除消息失败：' + P.errorText(err), true);
      });
    }

    if ($(opts.refresh)) $(opts.refresh).addEventListener('click', function () { load(); });

    return { load: load, render: render, state: state };
  }

  global.PAIGU_MSGLOG = { create: create };
})(window);
