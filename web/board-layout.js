(function (P) {
  'use strict';
  var storageKey = 'paigu.board-width';
  var mode = 'compact';
  var listeners = [];
  var measures = new Map();
  try { if (localStorage.getItem(storageKey) === 'full') mode = 'full'; } catch (_) { /* optional storage */ }

  function notify() { listeners.forEach(function (fn) { fn(mode); }); }
  function setMode(value) {
    mode = value === 'full' ? 'full' : 'compact';
    try { localStorage.setItem(storageKey, mode); } catch (_) { /* optional storage */ }
    notify();
  }
  function onMode(fn) { listeners.push(fn); }
  function bindControls(host) {
    if (!host) return;
    host.classList.add('board-width-controls');
    host.setAttribute('role', 'group');
    host.setAttribute('aria-label', '排表姓名宽度');
    ['compact', 'full'].forEach(function (value) {
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'small';
      button.dataset.widthMode = value;
      button.textContent = value === 'full' ? '全长' : '三字宽';
      button.addEventListener('click', function () { setMode(value); });
      host.appendChild(button);
    });
    function update() {
      host.querySelectorAll('button').forEach(function (button) {
        var active = button.dataset.widthMode === mode;
        button.setAttribute('aria-pressed', String(active));
        button.classList.toggle('active', active);
      });
    }
    onMode(update);
    update();
  }

  function measure(host, text) {
    var font = '600 13px ' + getComputedStyle(host).fontFamily;
    var key = font + '|' + text;
    if (measures.has(key)) return measures.get(key);
    var probe = document.createElement('span');
    probe.style.cssText = 'position:fixed;visibility:hidden;white-space:pre;padding:0;border:0;letter-spacing:0;';
    probe.style.font = font;
    probe.textContent = text;
    document.body.appendChild(probe);
    var width = Math.ceil(probe.getBoundingClientRect().width);
    probe.remove();
    measures.set(key, width);
    return width;
  }

  function layout(host, rows) {
    var groups = Object.create(null);
    var queues = Object.create(null);
    rows.forEach(function (row) {
      var group = groups[row.item_id] || (groups[row.item_id] = Object.create(null));
      queues[row.item_id] = queues[row.item_id] || measure(host, mode === 'compact' ? '汉汉汉' : '汉');
      if (mode === 'full') row.singles.concat(row.waiting).forEach(function (person) {
        queues[row.item_id] = Math.max(queues[row.item_id], measure(host, person.display));
      });
      row.boxes.forEach(function (box) {
        var col = group[box.box_index] || (group[box.box_index] = { count: 1, nameWidth: measure(host, mode === 'compact' ? '汉汉汉' : '汉') });
        col.count = Math.max(col.count, row.capacity || 0);
        box.slots.forEach(function (slot) {
          col.count = Math.max(col.count, slot.index);
          if (mode === 'full' && slot.user && slot.status !== 'empty') col.nameWidth = Math.max(col.nameWidth, measure(host, slot.user));
        });
      });
    });
    var result = Object.create(null);
    Object.keys(groups).forEach(function (itemId) {
      var boxes = groups[itemId];
      var tracks = [], columns = Object.create(null), widths = Object.create(null);
      var total = 0;
      Object.keys(boxes).sort(function (a, b) { return +a - +b; }).forEach(function (boxId, index) {
        if (index) tracks.push('0px');
        var box = boxes[boxId];
        widths[boxId] = box.nameWidth + 6;
        columns[boxId] = tracks.length + 1;
        for (var slot = 0; slot < box.count; slot++) { tracks.push(widths[boxId] + 'px'); total += widths[boxId]; }
      });
      result[itemId] = { tracks: tracks.join(' '), columns: columns, widths: widths, queueWidth: queues[itemId] + 6, total: total + Math.max(0, tracks.length - 1) * 2 };
    });
    return result;
  }

  var reasons = {
    NewClaimFilled: {cls:'changed-new',icon:'▲',label:'新增填入'},
    CancelReleased: {cls:'changed-release',icon:'▼',label:'撤销释放'},
    AutoMovedForward: {cls:'changed-fwd',icon:'↷',label:'自动前移'},
    TailSegmentCreated: {cls:'changed-tail',icon:'⛓',label:'新包尾段'},
    TailSegmentUpdated: {cls:'changed-tail',icon:'⛓',label:'包尾更新'},
    AdminFixed: {cls:'changed-admin',icon:'✚',label:'管理员固定'},
    AdminUnlocked: {cls:'changed-admin',icon:'✚',label:'管理员解锁'},
    RecomputedByRuleChange: {cls:'changed-fwd',icon:'↻',label:'规则变更重算'}
  };
  function describe(cell, change) {
    var details = [cell.user || '空位', '第 ' + cell.box_index + ' 盒 · 第 ' + cell.index + ' 位'];
    if (cell.status === 'locked_empty') details.push('包尾锁定空位');
    else if (cell.status === 'locked') details.push('已锁定');
    if (cell.status === 'admin_reserved') details.push('管理员预留');
    if (cell.segment_id || /tail/i.test(cell.policy || '')) details.push('包尾');
    if (change) details.push((reasons[change.reason] ? reasons[change.reason].label : '排位变化') + '：' + (change.before || '空') + ' → ' + (change.after || '空'));
    return details.join('；');
  }

  window.addEventListener('storage', function (event) {
    if (event.key === storageKey) { mode = event.newValue === 'full' ? 'full' : 'compact'; notify(); }
  });
  if (document.fonts) document.fonts.ready.then(function () { measures.clear(); notify(); });
  P.boardLayout = { mode: function () { return mode; }, onMode: onMode, bindControls: bindControls,
    measure: measure, layout: layout, describe: describe, reasons: reasons,
    boxColor: function (index) { return 'hsl(' + ((Number(index) || 0) * 67 % 360) + ' 72% 55%)'; } };
})(window.PAIGU);
