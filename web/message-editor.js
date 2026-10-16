(function () {
  'use strict';
  var P = window.PAIGU;
  var titles = { Applied: '已记录', Rejected: '已拒绝', NeedConfirm: '待确认', Ignored: '已忽略', Error: '处理失败', Merged: '已合并', Duplicate: '已去重' };
  function copy(value) { return JSON.parse(JSON.stringify(value)); }
  function id() { return window.crypto.randomUUID(); }
  function el(tag, cls, text) { var n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; }
  function select(options, value, label, onChange) {
    var s = el('select'); s.setAttribute('aria-label', label);
    options.forEach(function (o) { var n = el('option', '', o[1]); n.value = o[0]; s.appendChild(n); });
    s.value = value; s.addEventListener('change', function () { onChange(s.value); }); return s;
  }
  function button(label, action, cls) { var b = el('button', cls || '', label); b.type = 'button'; b.addEventListener('click', action); return b; }
  function field(label, control) { var l = el('label', 'ae-field'); l.appendChild(el('span', '', label)); l.appendChild(control); return l; }

  P.openMessageEditor = function (opts) {
    if (document.querySelector('.modal-backdrop')) return;
    var wrap = el('div', 'modal-backdrop action-editor-backdrop');
    var modal = el('div', 'modal action-editor'); modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true'); modal.setAttribute('aria-labelledby', 'action-editor-title');
    modal.innerHTML = '<header class="ae-head"><h2 id="action-editor-title">修订消息动作</h2><button type="button" class="ov-cancel" aria-label="关闭消息编辑">关闭</button></header>' +
      '<div class="ae-loading" role="status">正在读取原结果与当前动作…</div><div class="ae-content" hidden></div>' +
      '<div class="modal-err" role="alert"></div><footer class="modal-actions"><span class="ae-save-hint">仅保存本次修订，保留原消息与后续操作</span><button type="button" class="ov-ok" disabled>保存并重算</button></footer>';
    wrap.appendChild(modal); document.body.appendChild(wrap);
    var beforeFocus = document.activeElement, closed = false, saving = false, context, actions = [], treatment = 'replace', note;
    var save = modal.querySelector('.ov-ok'), error = modal.querySelector('.modal-err'), closeButton = modal.querySelector('.ov-cancel');
    var transaction = id(), lastRequest = null;
    closeButton.focus();
    function close() { if (saving) return; closed = true; wrap.remove(); if (beforeFocus && beforeFocus.isConnected) beforeFocus.focus(); }
    closeButton.addEventListener('click', close);
    wrap.addEventListener('click', function (e) { if (e.target === wrap) close(); });
    wrap.addEventListener('keydown', function (e) {
      if (e.key === 'Escape') { e.preventDefault(); close(); return; }
      if (e.key !== 'Tab') return;
      var nodes = Array.from(modal.querySelectorAll('input,select,textarea,button,summary')).filter(function (n) { return !n.disabled && n.getClientRects().length; });
      var first = nodes[0], last = nodes[nodes.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    });
    modal.addEventListener('input', function () { error.textContent = ''; });
    var endpoint = opts.apiBase + '/api/messages/' + encodeURIComponent(opts.message.record_id) + '/actions';
    P.get(endpoint + '?round_id=' + encodeURIComponent(opts.message.round_id)).then(function (data) {
      if (closed) return;
      context = data;
      modal.querySelector('.ae-loading').hidden = true;
      var content = modal.querySelector('.ae-content'); content.hidden = false;
      var src = el('section', 'ae-original');
      src.appendChild(el('b', '', opts.message.display + ' · #' + data.message.seq));
      src.appendChild(el('p', 'ae-raw', data.message.text));
      src.appendChild(el('p', 'hint', '当前：' + (titles[data.current_outcome.status] || data.current_outcome.status) + ' · ' + (data.current_outcome.summary || data.current_outcome.detail || '')));
      content.appendChild(src);
      var compare = el('details', 'ae-compare'), summary = el('summary', '', '查看原自动结果'); compare.appendChild(summary);
      if (data.original_automatic) {
        var original = data.original_automatic.original_outcome;
        summary.textContent += ' · ' + (titles[original.status] || original.status);
        compare.appendChild(el('p', '', original.status === 'Applied' && data.original_action_summary ? data.original_action_summary : original.detail || '无说明'));
        if (original.status !== 'Applied' && data.original_action_summary) compare.appendChild(el('p', '', '理解动作：' + data.original_action_summary));
        if (original.reply && original.reply !== original.detail) compare.appendChild(el('p', '', original.reply));
        compare.appendChild(el('p', 'hint', '自动理解保留不变；当前执行结果可能因修订或权限变化而不同。'));
      } else { compare.appendChild(el('p', '', '此历史消息没有可核对的原自动基线，不会伪造或重新解析。')); }
      content.appendChild(compare);
      if (!data.editable) {
        content.appendChild(el('p', 'ae-warning', '此消息属于自动合并组，须整组修订；完整组事务尚未开放，本次不会修改任何消息。'));
        data.automatic_group.members.forEach(function (m) { content.appendChild(el('p', '', '#' + m.seq + ' ' + m.text)); });
        return;
      }
      var draft = copy(data.editable_selection); treatment = draft.kind; actions = draft.actions || [];
      var operation = select([['replace','编辑动作'],['ignore','人工忽略'],['reject','人工拒绝'],['automatic','恢复自动结果']], treatment, '处理方式', function (v) { treatment = v; drawActions(); });
      operation.className = 'ae-treatment';
      operation.querySelector('option[value="automatic"]').disabled = !data.can_restore_automatic;
      content.appendChild(field('本次处理', operation));
      var body = el('section', 'ae-actions'); content.appendChild(body);
      note = el('textarea', 'ae-note'); note.value = draft.note || ''; note.rows = 2; note.maxLength = 2000;
      note.placeholder = '说明当时的意思或修订原因，便于复盘（可选）';
      content.appendChild(field('管理员解释', note));
      var index = data.catalog || [];
      function newLine() { return { line_id: id(), item_id: '', subitem_id: null, quantity: 1, claim_type: 'Split', slot_policy: 'Normal', is_proxy_card: false, notes: null }; }
      function itemLabel(item, i) { var dup = index.filter(function (v) { return v.name === item.name; }).length > 1; return item.name + (dup ? '（' + item.kind + '·第' + (i + 1) + '项）' : ''); }
      function itemInput(line) {
        var row = el('div', 'ae-item-row');
        var product = el('input', 'ov-item'), sub = el('input', 'ov-subitem'), qty = el('input', 'ov-qty');
        var listId = 'ae-products-' + id(), subId = 'ae-subitems-' + id(), list = el('datalist'), subList = el('datalist');
        list.id = listId; subList.id = subId; product.setAttribute('list', listId); sub.setAttribute('list', subId);
        product.placeholder = '搜索商品'; sub.placeholder = '子项（可选）';
        product.autocomplete = sub.autocomplete = 'off';
        index.forEach(function (item, i) { var option = el('option'); option.value = itemLabel(item, i); list.appendChild(option); });
        var current = index.find(function (item) { return item.item_id === line.item_id; });
        product.value = current ? itemLabel(current, index.indexOf(current)) : (line.item_id ? '原商品已移除' : '');
        function subOptions() {
          subList.replaceChildren();
          current = index.find(function (item) { return item.item_id === line.item_id; });
          (current && current.subitems || []).forEach(function (s) { var o = el('option'); o.value = s.name; subList.appendChild(o); });
          var found = (current && current.subitems || []).find(function (s) { return s.subitem_id === line.subitem_id; });
          sub.value = found ? found.name : (line.subitem_id ? '原子项已移除' : '');
        }
        subOptions();
        product.addEventListener('input', function () {
          var value = product.value.trim();
          var matches = index.filter(function (item, i) { return itemLabel(item, i) === value || item.name === value || (item.aliases || []).indexOf(value) >= 0; });
          var match = matches.length === 1 ? matches[0] : null;
          if (!match) { line.item_id = ''; return; }
          if (line.item_id !== match.item_id) {
            line.item_id = match.item_id; line.subitem_id = null;
            line.claim_type = /^(特典|gift)$/i.test(match.kind) ? 'GiftClaim' : /^(拼团|group|split)$/i.test(match.kind) ? 'Split' : 'Single';
            typeSelect.value = line.claim_type; subOptions();
          }
        });
        sub.addEventListener('input', function () {
          var matches = (current && current.subitems || []).filter(function (s) { return s.name === sub.value.trim() || (s.aliases || []).indexOf(sub.value.trim()) >= 0; });
          line.subitem_id = sub.value.trim() ? matches.length === 1 ? matches[0].subitem_id : '__invalid_subitem__' : null;
        });
        qty.type = 'number'; qty.min = '1'; qty.max = '99'; qty.value = line.quantity;
        qty.addEventListener('input', function () { line.quantity = Number(qty.value); });
        row.appendChild(field('商品', product)); row.appendChild(field('子项', sub)); row.appendChild(field('数量', qty)); row.appendChild(list); row.appendChild(subList);
        var more = el('details', 'ae-item-options'); more.appendChild(el('summary', '', '认购策略'));
        var optionGrid = el('div', 'ae-options-grid'); more.appendChild(optionGrid);
        var typeSelect = select([['Split','拼团'],['Single','单领／整盒'],['GiftClaim','特典']], line.claim_type, '认购类型', function (v) { line.claim_type = v; });
        optionGrid.appendChild(field('认购类型', typeSelect));
        var policies = [['Normal','普通排位'],['TailLocked','包尾'],['FullBox','包盒']];
        if (line.slot_policy === 'ColumnLocked') policies.push(['ColumnLocked','原锁列（现按普通处理）']);
        if (line.slot_policy === 'AdminFixed') policies.push(['AdminFixed','原管理员固定位（须另行处理）']);
        optionGrid.appendChild(field('位置策略', select(policies, line.slot_policy, '位置策略', function (v) { line.slot_policy = v; })));
        var proxy = el('input'); proxy.type = 'checkbox'; proxy.checked = line.is_proxy_card; proxy.addEventListener('change', function () { line.is_proxy_card = proxy.checked; });
        optionGrid.appendChild(field('代卡标记', proxy));
        var extra = el('input'); extra.value = line.notes || ''; extra.maxLength = 1000; extra.addEventListener('input', function () { line.notes = extra.value || null; }); optionGrid.appendChild(field('商品备注', extra));
        row.appendChild(more); return row;
      }
      function drawActions() {
        var active = document.activeElement;
        var redrawFocus = body.contains(active), activeCard = active && active.closest('.ae-action');
        var activeId = activeCard && activeCard.dataset.actionId;
        var activeLabel = active && active.getAttribute('aria-label');
        function restoreEditFocus() {
          if (!redrawFocus) return;
          var cards = Array.from(body.querySelectorAll('.ae-action'));
          var card = cards.find(function (c) { return c.dataset.actionId === activeId; }) || cards[cards.length - 1];
          var target = card && activeLabel ? Array.from(card.querySelectorAll('[aria-label]')).find(function (n) { return n.getAttribute('aria-label') === activeLabel; }) : null;
          target = target || (card && card.querySelector('.ov-item,select')) || body.querySelector('.ae-add-action') || operation;
          if (target) target.focus();
        }
        body.replaceChildren();
        if (treatment !== 'replace') {
          body.appendChild(el('p', 'ae-warning', treatment === 'automatic' ? '保存后改用已存自动结果，按当前权限重算；不会重新问模型。' : '保存后本条不执行排位动作。原文、原自动结果和解释仍保留。'));
          restoreEditFocus();
          return;
        }
        if (!actions.length) body.appendChild(el('p', 'hint', '当前没有可直接带入的动作。可添加动作，或保存空动作集（不执行排位动作）。'));
        actions.forEach(function (action, ai) {
          var card = el('div', 'ae-action'), head = el('div', 'ae-action-head');
          card.dataset.actionId = action.action_id;
          head.appendChild(el('b', '', '动作 ' + (ai + 1)));
          head.appendChild(select([['claim','排'],['cancel','撤'],['modify','改单（撤旧后重排）']], action.operation, '动作类型', function (v) { action.operation = v; if (v !== 'cancel' && !action.items.length) action.items.push(newLine()); drawActions(); }));
          var up = button('上移', function () { var a = actions[ai - 1]; actions[ai - 1] = action; actions[ai] = a; drawActions(); }); up.disabled = ai === 0;
          head.appendChild(up); head.appendChild(button('删除动作', function () { actions.splice(ai, 1); drawActions(); })); card.appendChild(head);
          if (action.operation === 'cancel') {
            var targetMode = action.target_claim_id ? 'claim' : action.target_item_id ? 'item' : 'latest';
            card.appendChild(field('撤销范围', select([['latest','最近一笔申购'],['item','指定商品'],['claim','具体申购']], targetMode, '撤销范围', function (v) {
              action.target_claim_id = v === 'claim' ? (data.cancel_targets[0] && data.cancel_targets[0].claim_id || '__choose_target__') : null;
              action.target_item_id = v === 'item' ? '__choose_item__' : null; action.quantity = null; drawActions();
            })));
            if (targetMode === 'claim') {
              var targets = [['__choose_target__','请选择原申购']].concat(data.cancel_targets.map(function (t) { return [t.claim_id, '#' + t.message_seq + ' · ' + t.summary]; }));
              if (!targets.some(function (t) { return t[0] === action.target_claim_id; })) targets.push([action.target_claim_id,'原目标已失效（保留此引用，不改撤别单）']);
              card.appendChild(field('原申购', select(targets, action.target_claim_id, '原申购', function (v) { action.target_claim_id = v; })));
            } else if (targetMode === 'item') {
              card.appendChild(field('商品', select([['__choose_item__','请选择商品']].concat(index.map(function (item, i) { return [item.item_id,itemLabel(item,i)]; })), action.target_item_id, '撤销商品', function (v) { action.target_item_id = v; })));
              var n = el('input', 'ae-cancel-quantity'); n.type = 'number'; n.min = '1'; n.max = '99'; n.placeholder = '全部'; n.value = action.quantity == null ? '' : action.quantity;
              n.addEventListener('input', function () { action.quantity = n.value === '' ? null : Number(n.value); }); card.appendChild(field('撤销份数（空白＝全部）', n));
            }
          } else {
            action.items.forEach(function (line, li) {
              var row = itemInput(line);
              row.appendChild(button('移除商品项', function () { action.items.splice(li, 1); drawActions(); }, 'ae-remove-item'));
              card.appendChild(row);
            });
            card.appendChild(button('＋ 商品项', function () { action.items.push(newLine()); drawActions(); }, 'ae-add-item'));
          }
          body.appendChild(card);
        });
        body.appendChild(button('＋ 添加动作', function () { actions.push({ action_id: id(), operation: 'claim', items: [newLine()], target_claim_id: null, target_item_id: null, quantity: null }); drawActions(); }, 'ae-add-action'));
        restoreEditFocus();
      }
      drawActions(); save.disabled = false;
      var focus = modal.querySelector('.ov-item') || operation; focus.focus();
    }).catch(function (e) { if (!closed) {
      modal.querySelector('.ae-loading').textContent = '读取失败，未修改消息。';
      error.textContent = e.status === 404 || e.status === 405 ? '服务暂不支持此编辑接口，或消息已不在当前数据中。请关闭并刷新核对。' : P.errorText(e);
    } });

    save.addEventListener('click', function () {
      if (saving || !context || !context.editable) return;
      var selection = { kind: treatment, note: note.value };
      if (treatment === 'replace') {
        selection.actions = actions.map(function (a) {
          return { action_id: a.action_id, operation: a.operation, items: a.operation === 'cancel' ? [] : a.items,
            target_claim_id: a.operation === 'cancel' ? a.target_claim_id : null, target_item_id: a.operation === 'cancel' ? a.target_item_id : null,
            quantity: a.operation === 'cancel' ? a.quantity : null };
        });
        for (var a of selection.actions) {
          if (a.operation !== 'cancel') {
            if (!a.items.length) { error.textContent = '排/改须至少选择一个商品项'; return; }
            for (var line of a.items) {
              if (!line.item_id) { error.textContent = '请从候选中选择商品'; return; }
              if (!Number.isInteger(line.quantity) || line.quantity < 1 || line.quantity > 99) { error.textContent = '数量应为 1–99 的整数'; return; }
            }
          }
        }
      }
      var signature = JSON.stringify(selection);
      if (lastRequest && lastRequest !== signature) transaction = id();
      lastRequest = signature;
      saving = true; error.textContent = ''; save.disabled = true; save.textContent = '正在保存…';
      var controls = Array.from(modal.querySelectorAll('input,select,textarea,button'));
      var disabled = controls.map(function (n) { return n.disabled; }); controls.forEach(function (n) { n.disabled = true; });
      P.post(endpoint, { transaction_id: transaction, round_id: context.round_id, config_revision: context.config_revision, record_revision: context.message.record_revision, selection: selection }).then(function (response) {
        if (closed) return;
        if (opts.onSaved) opts.onSaved(response);
        saving = false; close(); if (P.toast) P.toast(response.deduplicated ? '这笔修订已保存，当前结果已刷新' : '已保存并重算');
      }).catch(function (e) {
        if (closed) return;
        saving = false; controls.forEach(function (n, i) { n.disabled = disabled[i]; }); save.disabled = false; save.textContent = '保存并重算';
        var code = e.data && e.data.error;
        var reasons = { stale_record_revision: '此消息已被其他操作修改；本次草稿保留，请关闭后重新核对。', stale_revision: '配置已变化；本次草稿保留，请重新核对商品与权限后再保存。', round_changed: '轮次已切换，本次修改未保存。', locked: '轮次已锁定，本次修改未保存。' };
        error.textContent = reasons[code] || (e.data && e.data.message) || (typeof code === 'string' ? code : P.errorText(e));
        error.tabIndex = -1; error.focus({ preventScroll: true });
      });
    });
  };
})();
