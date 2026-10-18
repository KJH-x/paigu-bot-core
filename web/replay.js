(function () {
  'use strict';

  var P = window.PAIGU;
  var esc = P.esc;

  function $(id) { return document.getElementById(id); }


  var KIND_LABEL = { split: '拼团', group: '拼团', single: '单领', box: '整盒', whole_box: '整盒', gift: '特典' };

  var state = {
    data: null,
    source: '',
    current: 0,
    playing: false,
    speed: 1,
    timer: null,
    view: 'table',
    cumulative: [],
    lastChange: {},
    focusedKey: null,
    meta: null,
    mermaidReady: false,
    graphItem: null,
    graphRequest: 0,
    graphFit: false,
    graphMatch: null,
    viewScroll: {},
    graphRestoreTop: null,
    resetColumn: true,
    wwOpen: true,
    changes: [],
    active: true,
    inFlight: false,
    workflowStamp: '',
    requestId: 0,
    queuedLoad: false,
    expectedRound: ''
  };

  var tableEl, boardRenderer;

  function boardKey(itemId, subitemId) {
    return itemId + '|' + (subitemId == null ? '' : subitemId);
  }

  function cellKey(key, slot) {
    return key + '#1:' + slot;
  }

  function kindLabel(k) { return KIND_LABEL[k] || k || ''; }

  function msgBySeq(seq) {
    var msgs = (state.data && state.data.messages) || [];
    for (var i = 0; i < msgs.length; i++) {
      if (msgs[i].seq === seq) return msgs[i];
    }
    return null;
  }

  function buildDerived() {
    var steps = (state.data && state.data.steps) || [];
    var boards = [];
    var cur = {};
    var lastChange = {};
    var changes = [];
    var previousRows = [];
    var widthRows = [];
    for (var i = 0; i < steps.length; i++) {
      var next = {};
      if (!Array.isArray(cur.item_allocations)) {
        for (var k in cur) { if (Object.prototype.hasOwnProperty.call(cur, k) && Array.isArray(cur[k])) next[k] = cur[k].slice(); }
      }
      var b = steps[i].board || {};
      if (Array.isArray(b.item_allocations)) {
        next = b;
      } else {
        for (var k2 in b) {
          if (!Object.prototype.hasOwnProperty.call(b, k2) || !Array.isArray(b[k2])) continue;
          next[k2] = b[k2].slice();
        }
      }
      var changed = steps[i].changed || [];
      for (var c = 0; c < changed.length; c++) {
        lastChange[cellKey(boardKey(changed[c].item_id, changed[c].subitem_id), changed[c].slot)] = i;
      }
      boards.push(next);
      var rows = P.normalizeBoard({ board: next }, state.meta);
      widthRows = widthRows.concat(rows);
      var map = steps[i].changed ? P.normalizeChanged(steps[i]) : P.diffBoardItems(previousRows, rows);
      if (!steps[i].changed) {
        steps[i].changed = Object.keys(map).map(function (key) {
          var parts = key.slice(0, key.lastIndexOf('#')).split('|');
          var position = key.slice(key.lastIndexOf('#') + 1).split(':');
          return {item_id:parts[0], subitem_id:parts.slice(1).join('|'), box_index:+position[0],
            slot:+position[1], before:map[key].before, after:map[key].after, reason:map[key].reason};
        });
      }
      Object.keys(map).forEach(function (key) { lastChange[key] = i; });
      changes.push(map);
      previousRows = rows;
      cur = next;
    }
    state.cumulative = boards;
    state.lastChange = lastChange;
    state.changes = changes;
    boardRenderer.setWidthSource(widthRows.concat(P.normalizeBoard({board:state.data.board || {}}, state.meta)));
  }

  function renderHeader() {
    if (!state.data) return;
    var steps = state.data.steps || [];
    $('replay-title').textContent = state.data.title || state.data.sample_id || '排谷重放';
    $('step-info').textContent = state.data.latest_only ? '当前排位 · 逐步历史尚未启用' : '共 ' + steps.length + ' 步 · 当前第 ' +
      (steps.length ? state.current + 1 : 0) + ' 步';
    $('step-count').textContent = state.data.latest_only ? '仅当前快照' : (steps.length ? (state.current + 1) + '/' + steps.length : '0/0');
    $('source-info').textContent = state.active ? '当前激活轮次 · 版本 ' + state.data.version : '导入文件 · ' + state.source;
    var step=steps[state.current], message=step && msgBySeq(step.message_seq), process=$('process-message');
    process.disabled=!(state.active && message && message.record_id && state.data.round_id);
    process.title=state.active?'复盘保持只读；在排谷打开同一稳定记录，不自动保存':'导入文件只读，不连接本机写入记录';
  }

  function renderList() {
    var ul = $('step-list');
    var steps = state.data.steps || [];
    P.syncKeyedChildren(
      ul,
      steps,
      function (s, i) { return 'step' + i; },
      function () {
        var li = document.createElement('li');
        li.className = 'step-item';
        li.innerHTML = '<button class="step-open" type="button"><span class="seq"></span><span class="who"></span>' +
          '<span class="txt"></span><span class="status"></span></button>';
        li.__seq = li.querySelector('.seq');
        li.__who = li.querySelector('.who');
        li.__txt = li.querySelector('.txt');
        li.__st = li.querySelector('.status');
        return li;
      },
      function (li, step, key) {
        var i = parseInt(key.slice(4), 10);
        var msg = msgBySeq(step.message_seq);
        var seq = msg ? msg.seq : step.message_seq;
        var display = msg ? (msg.display || msg.user) : '?';
        var text = msg ? msg.text : '';
        var status = msg ? (msg.status || '') : '';
        li.className = 'step-item' + (i === state.current ? ' current' : '');
        li.__seq.textContent = '#' + seq;
        li.__who.textContent = display;
        li.__txt.textContent = text;
        li.__txt.title = text;
        li.__st.textContent = ({Applied:'已记录', Rejected:'已拒绝', Ignored:'已忽略', Duplicate:'已去重', Merged:'已合并', NeedConfirm:'待确认', Error:'错误'})[status] || status;
        li.__st.className = 'status' + (status ? ' st-' + String(status).toLowerCase() : '');
        li.querySelector('button').setAttribute('aria-label','查看第 '+(i+1)+' 步：'+display+'，'+text);
        if(i===state.current)li.querySelector('button').setAttribute('aria-current','step');else li.querySelector('button').removeAttribute('aria-current');
        li.onclick = function () { setPlaying(false); goToStep(i); };
      }
    );
    var curEl = ul.querySelector('.current');
    if (curEl && curEl.scrollIntoView) curEl.scrollIntoView({ block: 'nearest' });
  }

  function applyFocus() {
    var cells = tableEl.querySelectorAll('.cell');
    for (var i = 0; i < cells.length; i++) {
      if (cells[i].__k) cells[i].classList.toggle('focused', cells[i].__k === state.focusedKey);
    }
  }

  function renderBoard() {
    var step = (state.data.steps || [])[state.current];
    var board = state.cumulative[state.current] || state.data.board || {};
    var rows = P.normalizeBoard({ board: board }, state.meta);
    var changeMap = state.changes[state.current] || {};
    boardRenderer.render(rows, changeMap);
    applyFocus();
  }

  function mermaidLabel(s) {
    return String(s == null ? '' : s)
      .replace(/#/g, '#35;')
      .replace(/["\[\]{}()<>|&]/g,function(character){return '#'+character.charCodeAt(0)+';';})
      .replace(/[\r\n]/g, ' ')
      .trim();
  }

  function buildMermaid(itemId) {
    var data = state.data;
    var step = (data.steps || [])[state.current];
    var changes = (step && step.changed) || [];
    var board = {};
    P.normalizeBoard({board: state.cumulative[state.current] || data.board || {}}, state.meta).forEach(function (row) {
      board[row.key] = [].concat.apply([], row.boxes.map(function (box) { return box.slots; })).concat(
        row.singles.map(function(entry){return {user:entry.display+' · 单领 ×'+entry.quantity,status:'filled'};}),
        row.waiting.map(function(entry){return {user:entry.display+' · 等待 ×'+entry.quantity,status:'filled'};})
      );
    });

    var changedItemIds = {};
    var changedSubitemKeys = {};
    var changedUserKeys = {};
    for (var i = 0; i < changes.length; i++) {
      var c = changes[i];
      var ck = boardKey(c.item_id, c.subitem_id);
      changedItemIds[c.item_id] = true;
      changedSubitemKeys[ck] = true;
      if (c.before) changedUserKeys[ck + '||' + c.before] = true;
      if (c.after) changedUserKeys[ck + '||' + c.after] = true;
    }

    var nodeLines = [];
    var edges = [];
    var hlNodes = {};
    var seq = 0;
    function nextId() { return 'n' + (seq++); }
    var itemIds = {};
    var subitemIds = {};
    var userIds = {};
    var items = (data.items || []).filter(function(item){return item.item_id===itemId;});

    for (var a = 0; a < items.length; a++) {
      var item = items[a];
      var iid = itemIds[item.item_id];
      if (!iid) { iid = nextId(); itemIds[item.item_id] = iid; }
      nodeLines.push(iid + '["' + mermaidLabel(item.name) + '"]');
      if (changedItemIds[item.item_id]) hlNodes[iid] = true;

      var hasSubitems = !!(item.subitems && item.subitems.length);
      var subitems = hasSubitems
        ? item.subitems
        : [{ subitem_id: '', name: (item.kind === 'single' ? '单领' : '默认'), capacity: 0 }];

      for (var v = 0; v < subitems.length; v++) {
        var subitem = subitems[v];
        var key = boardKey(item.item_id, subitem.subitem_id);
        var slots = board[key] || [];
        var users = [];
        var seen = {};
        for (var s = 0; s < slots.length; s++) {
          var u = slots[s] && slots[s].user;
          if (u && slots[s].status !== 'empty' && !seen[u]) { seen[u] = true; users.push(u); }
        }

        if (hasSubitems) {
          var vid = subitemIds[key];
          if (!vid) { vid = nextId(); subitemIds[key] = vid; }
          nodeLines.push(vid + '{{"' + mermaidLabel(subitem.name) + '"}}');
          if (changedSubitemKeys[key]) hlNodes[vid] = true;
          edges.push({ from: iid, to: vid, changed: !!changedSubitemKeys[key] });
          for (var uu = 0; uu < users.length; uu++) {
            var uk = key + '||' + users[uu];
            var uid = userIds[uk];
            if (!uid) { uid = nextId(); userIds[uk] = uid; }
            nodeLines.push(uid + '(["' + mermaidLabel(users[uu]) + '"])');
            var chU = !!changedUserKeys[uk];
            if (chU) hlNodes[uid] = true;
            edges.push({ from: vid, to: uid, changed: chU });
          }
        } else {
          for (var u2 = 0; u2 < users.length; u2++) {
            var uk2 = key + '||' + users[u2];
            var uid2 = userIds[uk2];
            if (!uid2) { uid2 = nextId(); userIds[uk2] = uid2; }
            nodeLines.push(uid2 + '(["' + mermaidLabel(users[u2]) + '"])');
            var chU2 = !!changedUserKeys[uk2];
            if (chU2) hlNodes[uid2] = true;
            edges.push({ from: iid, to: uid2, changed: chU2 });
          }
        }
      }
    }

    if (!nodeLines.length) return 'graph LR\n  empty["无商品数据"]';

    var lines = ['graph LR'].concat(nodeLines);
    var hlEdges = [];
    for (var e = 0; e < edges.length; e++) {
      lines.push(edges[e].from + ' --> ' + edges[e].to);
      if (edges[e].changed) hlEdges.push(e);
    }
    lines.push('classDef hlNode fill:#ffe08a,stroke:#d97706,stroke-width:3px,color:#7c2d12;');
    var hlIds = Object.keys(hlNodes);
    if (hlIds.length) lines.push('class ' + hlIds.join(',') + ' hlNode;');
    if (hlEdges.length) lines.push('linkStyle ' + hlEdges.join(',') + ' stroke:#dc2626,stroke-width:3px;');
    return lines.join('\n');
  }

  function graphItems() {
    var items=state.data.items || [];
    if(state.graphItem===null || (state.graphItem!=='' && !items.some(function(item){return item.item_id===state.graphItem;}))){
      var step=(state.data.steps || [])[state.current],changed=step && step.changed && step.changed[0];
      state.graphItem=(changed && items.some(function(item){return item.item_id===changed.item_id;}) && changed.item_id) || (items[0] && items[0].item_id) || '';
    }
    var select=$('graph-item'),signature=JSON.stringify(items.map(function(item){return [item.item_id,item.name];}));
    if(select.dataset.signature!==signature){
      select.dataset.signature=signature;select.replaceChildren();
      [['','全部商品（分组）']].concat(items.map(function(item){return [item.item_id,item.name || item.item_id];})).forEach(function(pair){var option=document.createElement('option');option.value=pair[0];option.textContent=pair[1];select.appendChild(option);});
    }
    select.value=state.graphItem;
    return state.graphItem==='' ? items : items.filter(function(item){return item.item_id===state.graphItem;});
  }

  function sizeGraph(card) {
    var svg=card.querySelector('svg'),canvas=card.querySelector('.graph-canvas');
    if(!svg)return;
    var width=parseFloat(svg.getAttribute('width')) || svg.viewBox.baseVal.width;
    var scale=state.graphFit ? Math.max(1,Math.min(1.25,canvas.clientWidth/width)) : 1;
    svg.style.width=(width*scale)+'px';svg.style.maxWidth='none';svg.style.height='auto';
  }

  function renderMermaid() {
    var host=$('mermaid-view'),request=++state.graphRequest,items=graphItems(),parent=document.querySelector('.replay-content');
    var top=state.graphRestoreTop==null ? parent.scrollTop : state.graphRestoreTop,left=parent.scrollLeft;
    state.graphRestoreTop=null;
    if(!window.mermaid){host.textContent='本地绘图暂不可用，仍可使用完整表格或列视图核对。';return;}
    if(!state.mermaidReady){window.mermaid.initialize({startOnLoad:false,securityLevel:'strict'});state.mermaidReady=true;}
    P.syncKeyedChildren(host,items,function(item){return item.item_id;},function(){
      var card=document.createElement('section');card.className='graph-card';
      var heading=document.createElement('h3'),title=document.createElement('b'),kind=document.createElement('span'),canvas=document.createElement('div');
      heading.append(title,kind);canvas.className='graph-canvas';canvas.tabIndex=0;canvas.setAttribute('role','region');card.append(heading,canvas);return card;
    },function(card,item){
      card.dataset.item=item.item_id;card.querySelector('b').textContent=item.name || item.item_id;
      card.querySelector('h3 span').textContent=kindLabel(item.kind);
      var canvas=card.querySelector('.graph-canvas'),scroll=canvas.scrollLeft;
      canvas.setAttribute('aria-label',(item.name || item.item_id)+'关系图；超宽可横向滚动');
      canvas.setAttribute('aria-busy','true');
      var rid='relation-'+request+'-'+Math.random().toString(36).slice(2);
      Promise.resolve().then(function(){return window.mermaid.render(rid,buildMermaid(item.item_id));}).then(function(result){
        if(request!==state.graphRequest || state.view!=='mermaid' || !card.isConnected)return;
        canvas.innerHTML=result.svg;canvas.setAttribute('aria-busy','false');
        var svg=canvas.querySelector('svg');svg.setAttribute('aria-label',(item.name || item.item_id)+'当前步骤关系图');
        sizeGraph(card);canvas.scrollLeft=scroll;
        if(state.graphMatch && state.graphMatch.item===item.item_id){
          var match=Array.from(canvas.querySelectorAll('.graph-node')).find(function(node){return node.dataset.graphLabel===state.graphMatch.label;});
          if(match)match.classList.add('located');
        }
        parent.scrollTop=top;parent.scrollLeft=left;
      }).catch(function(error){
        if(request!==state.graphRequest || !card.isConnected)return;
        canvas.setAttribute('aria-busy','false');canvas.textContent='绘图失败：'+(error.message || error)+'；可切换表格或列视图核对。';
      });
    });
    $('graph-status').textContent=items.length+' 个商品分组 · '+(state.graphFit?'适配宽度，不缩小字号':'原始字号')+' · 定位不改变步骤';
  }

  function locateGraph() {
    var query=$('graph-find').value.trim().toLocaleLowerCase();
    if(!query){$('graph-status').textContent='请输入当前商品中的姓名或款式；需要跨商品时先选“全部商品”。';$('graph-find').focus();return;}
    var matches=Array.from($('mermaid-view').querySelectorAll('.graph-node')).filter(function(node){return node.dataset.graphLabel.toLocaleLowerCase().includes(query);});
    if(!matches.length){$('graph-status').textContent='当前商品/步骤没有匹配项；可切换商品或步骤，不会自动跳步。';return;}
    var previous=matches.findIndex(function(node){return node.classList.contains('located');}),node=matches[(previous+1)%matches.length];
    $('mermaid-view').querySelectorAll('.located').forEach(function(n){n.classList.remove('located');});node.classList.add('located');
    state.graphMatch={item:node.closest('.graph-card').dataset.item,label:node.dataset.graphLabel};
    node.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'});node.focus({preventScroll:true});
    $('graph-status').textContent='匹配 '+((previous+1)%matches.length+1)+' / '+matches.length+'：'+node.dataset.graphLabel+'；复盘步骤不变';
  }

  function fitGraph(fit) {
    state.graphFit=fit;$('mermaid-view').querySelectorAll('.graph-card').forEach(function(card){sizeGraph(card);card.querySelector('.graph-canvas').scrollLeft=0;});
    $('graph-status').textContent=fit?'已适配宽度；不缩小字号，超宽可在图内横滚':'已恢复原始字号';
  }

  function renderColumn() {
    var root = $('column-view');
    var oldWrap=state.resetColumn ? null : root.querySelector('.col-wrap');
    var oldLeft=oldWrap ? oldWrap.scrollLeft : 0,oldTop=oldWrap ? oldWrap.scrollTop : 0;
    state.resetColumn=false;
    var steps = state.data.steps || [];
    var items = state.data.items || [];

    var rows = [];
    for (var i = 0; i < items.length; i++) {
      var it = items[i];
      if (it.subitems && it.subitems.length) {
        for (var v = 0; v < it.subitems.length; v++) {
          rows.push({ key: boardKey(it.item_id, it.subitems[v].subitem_id), label: it.name + ' / ' + it.subitems[v].name });
        }
      } else {
        rows.push({ key: boardKey(it.item_id, ''), label: it.name });
      }
    }

    var stepCells = [];
    for (var j = 0; j < steps.length; j++) {
      var map = {};
      var slotMap = {};
      P.normalizeBoard({board:state.cumulative[j]}, state.meta).forEach(function (row) {
        row.boxes.forEach(function (box) { box.slots.forEach(function (slot) {
          slotMap[row.key + '#' + box.box_index + ':' + slot.index] = slot;
        }); });
      });
      Object.keys(state.changes[j] || {}).forEach(function (key) {
        var change = state.changes[j][key];
        if (change.after) {
          var pos = key.slice(key.lastIndexOf('#') + 1).split(':');
          var slot = slotMap[key] || {};
          map[key.split('#')[0]] = {user:change.after, box_index:+pos[0], index:+pos[1], change:change,
            status:slot.status,policy:slot.policy,segment_id:slot.segment_id};
        }
      });
      stepCells.push(map);
    }

    var widths = steps.map(function (step, index) {
      var msg = msgBySeq(step.message_seq);
      var nameWidth = P.boardLayout.measure(root, P.boardLayout.mode() === 'compact' ? '汉汉汉' : ((msg && (msg.display || msg.user)) || '汉'));
      if (P.boardLayout.mode() === 'full') Object.keys(stepCells[index]).forEach(function (key) {
        nameWidth = Math.max(nameWidth, P.boardLayout.measure(root, stepCells[index][key].user));
      });
      return nameWidth + 6;
    });
    var totalWidth = 220 + widths.reduce(function (sum, width) { return sum + width; }, 0);
    var html = '<div class="col-wrap"><table class="col-table" style="width:' + totalWidth + 'px"><colgroup><col style="width:220px">' +
      widths.map(function (width) { return '<col style="width:' + width + 'px">'; }).join('') + '</colgroup><thead><tr>' +
      '<th class="rowhead">商品 / 子项</th>';
    for (var j2 = 0; j2 < steps.length; j2++) {
      var msg = msgBySeq(steps[j2].message_seq);
      var disp = msg ? (msg.display || msg.user) : ('#' + steps[j2].message_seq);
      var cur = (j2 === state.current) ? ' current' : '';
      html += '<th class="colhead' + cur + '" scope="col" data-idx="' + j2 + '" title="' + esc(disp) + '：' +
        esc(msg ? msg.text : '') + '"><button class="column-step" type="button" aria-label="查看第 '+(j2+1)+' 步：'+esc(disp)+'"><span class="col-seq">#' + esc(steps[j2].message_seq) +
        '</span><span class="col-name">' + esc(disp) + '</span></button></th>';
    }
    html += '</tr></thead><tbody>';
    for (var r = 0; r < rows.length; r++) {
      html += '<tr data-row-key="'+esc(rows[r].key)+'"><th class="rowhead">' + esc(rows[r].label) + '</th>';
      for (var j3 = 0; j3 < steps.length; j3++) {
        var cell = stepCells[j3][rows[r].key];
        var user = cell && cell.user;
        var cls = 'colcell' + (j3 === state.current ? ' current' : '') + (user ? ' filled' : '');
        var reasonClass = cell ? (P.boardLayout.reasons[cell.change.reason] || P.boardLayout.reasons.NewClaimFilled).cls : '';
        var description = cell ? P.boardLayout.describe(cell, cell.change) : '本步无新增填入';
        var stateMark = cell && cell.status === 'locked' ? '<span class="cell-state lock-mark"></span>' :
          (cell && cell.status === 'admin_reserved' ? '<span class="cell-state reserve-mark"></span>' : '');
        if (cell && (cell.segment_id || /tail/i.test(cell.policy || ''))) stateMark += '<span class="cell-state tail-mark"></span>';
        html += '<td class="' + cls + '" data-idx="' + j3 + '"><button class="column-step cell ' + reasonClass + '" type="button" title="' + esc(description) + '" aria-label="查看第 '+(j3+1)+' 步：'+esc(description)+'" style="--bx:' + (cell ? P.boardLayout.boxColor(cell.box_index) : 'transparent') + '">' +
          '<span class="cell-box-mark" aria-hidden="true"></span><span class="cell-user">' + esc(user || '') + '</span><span class="cell-marks" aria-hidden="true">' + stateMark + '<span class="badge"' + (cell ? '' : ' hidden') + '></span></span></button></td>';
      }
      html += '</tr>';
    }
    html += '</tbody></table></div>';
    root.innerHTML = html;

    function bind(nodes) {
      for (var n = 0; n < nodes.length; n++) {
        nodes[n].addEventListener('click', function () {
          var index=parseInt(this.getAttribute('data-idx'),10),rowKey=this.closest('tr').dataset.rowKey;
          var restore=this.contains(document.activeElement);
          setPlaying(false);
          goToStep(index);
          if(restore){var row=Array.from(root.querySelectorAll('tr')).find(function(node){return node.dataset.rowKey===rowKey;});var button=row && row.querySelector('[data-idx="'+index+'"] button');if(button)button.focus({preventScroll:true});}
        });
      }
    }
    bind(root.querySelectorAll('th.colhead'));
    bind(root.querySelectorAll('td.colcell'));
    var curHead = root.querySelector('th.colhead.current');
    if(oldWrap){var wrap=root.querySelector('.col-wrap');wrap.scrollLeft=oldLeft;wrap.scrollTop=oldTop;}
    else if (curHead && curHead.scrollIntoView) curHead.scrollIntoView({ block: 'nearest', inline: 'center' });
  }

  function renderWhoWhats() {
    var panel = $('who-whats-panel');
    var step = state.data && (state.data.steps || [])[state.current];
    var list = P.normalizeWhoWhats({ who_whats: (step && step.who_whats) || (state.data && state.data.who_whats) || [] });
    if (!list.length) { panel.classList.add('hidden'); return; }
    panel.classList.remove('hidden');
    $('ww-count').textContent = list.length + ' 人';
    $('ww-title').textContent = (state.wwOpen ? '▾ ' : '▸ ') + (state.active ? '当前步成员明细' : '文件成员汇总');
    $('ww-body').classList.toggle('hidden', !state.wwOpen);
    P.syncKeyedChildren(
      $('who-whats-body'),
      list,
      function (w) { return w.display; },
      function () {
        var tr = document.createElement('tr');
        tr.innerHTML = '<td class="ww-display"></td><td class="ww-identity"></td><td class="ww-items"></td>';
        tr.__d = tr.querySelector('.ww-display');
        tr.__i = tr.querySelector('.ww-identity');
        tr.__it = tr.querySelector('.ww-items');
        return tr;
      },
      function (tr, w) {
        if (tr.__d.textContent !== w.display) tr.__d.textContent = w.display;
        if (tr.__i.textContent !== w.identity) tr.__i.textContent = w.identity;
        var detail = w.items.map(function (it) { return it.name + ' ×' + it.qty; }).join('、');
        if (tr.__it.textContent !== detail) tr.__it.textContent = detail;
      }
    );
  }

  function keyLabel(key) {
    var parts = key.split('|');
    var itemId = parts[0];
    var subitemId = parts.slice(1).join('|');
    var name = itemId;
    var items = (state.data && state.data.items) || [];
    for (var i = 0; i < items.length; i++) {
      if (items[i].item_id === itemId) { name = items[i].name; break; }
    }
    return subitemId ? name + ' / ' + subitemId : name + ' / 单领';
  }

  function renderCheck() {
    var bar = $('check-bar');
    var expected = state.data && state.data.expected;
    if (!expected || !Object.keys(expected).length) {
      bar.classList.add('hidden');
      bar.innerHTML = '';
      return;
    }
    bar.classList.remove('hidden');
    var board = state.cumulative.length ? (state.cumulative[state.cumulative.length - 1] || {}) : {};
    var diffs = [];
    var keys = Object.keys(expected);
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      var exp = expected[key] || [];
      var slots = (board[key] || []).slice().sort(function (a, b) { return a.slot - b.slot; });
      var actual = slots
        .filter(function (s) { return s.user && s.status !== 'empty'; })
        .map(function (s) { return s.user; });
      var same = exp.length === actual.length;
      if (same) {
        for (var j = 0; j < exp.length; j++) {
          if (exp[j] !== actual[j]) { same = false; break; }
        }
      }
      if (!same) diffs.push({ key: key, exp: exp, actual: actual });
    }

    if (!diffs.length) {
      bar.innerHTML = '<span class="pass">PASS</span>期望与当前最终态一致（共 ' +
        keys.length + ' 个分组）。';
      return;
    }

    var html = '<span class="fail">FAIL</span>期望与当前最终态存在 ' + diffs.length + ' 处差异：<ul class="diff-list">';
    for (var d = 0; d < diffs.length; d++) {
      html += '<li><span class="k">' + esc(keyLabel(diffs[d].key)) + '</span><br>' +
        '<span class="exp">期望：' + esc(diffs[d].exp.join(' → ') || '（空）') + '</span><br>' +
        '<span class="act">实际：' + esc(diffs[d].actual.join(' → ') || '（空）') + '</span></li>';
    }
    html += '</ul>';
    bar.innerHTML = html;
  }

  function renderRight() {
    var content=document.querySelector('.replay-content');content.style.setProperty('--replay-tabs-height',content.querySelector('.tabs').offsetHeight+'px');
    if(state.view!=='mermaid')state.graphRequest++;
    $('table-view').classList.toggle('hidden', state.view !== 'table');
    $('column-view').classList.toggle('hidden', state.view !== 'column');
    $('mermaid-view').classList.toggle('hidden', state.view !== 'mermaid');
    $('graph-tools').classList.toggle('hidden',state.view!=='mermaid');
    if (state.view === 'table') renderBoard();
    else if (state.view === 'column') renderColumn();
    else renderMermaid();
  }

  function renderAll() {
    renderHeader();
    renderList();
    renderRight();
    renderWhoWhats();
  }

  function goToStep(idx, opts) {
    if (!state.data) return;
    var steps = state.data.steps || [];
    if (!steps.length) return;
    idx = Math.max(0, Math.min(steps.length - 1, idx));
    state.current = idx;
    if (!opts || !opts.keepFocus) state.focusedKey = null;
    renderAll();
    updateControls();
  }

  function onCellClick(key) {
    var idx = state.lastChange[key];
    if (idx == null) {
      P.toast('该格在本次重放中未被修改');
      return;
    }
    state.focusedKey = key;
    setPlaying(false);
    goToStep(idx, { keepFocus: true });
    P.toast('已跳到第 ' + (idx + 1) + ' 步：最后一次修改该格的消息', 'ok');
  }

  function setPlaying(p) {
    state.playing = p;
    if (state.timer) { clearTimeout(state.timer); state.timer = null; }
    updateControls();
    if (p) scheduleTick();
  }

  function scheduleTick() {
    if (!state.playing) return;
    var delay = Math.max(120, 1000 / state.speed);
    state.timer = setTimeout(function () {
      if (!state.playing) return;
      var steps = state.data ? (state.data.steps || []) : [];
      if (state.current >= steps.length - 1) { setPlaying(false); return; }
      goToStep(state.current + 1);
      scheduleTick();
    }, delay);
  }

  function updateControls() {
    var steps = state.data ? (state.data.steps || []) : [];
    var has = steps.length > 0;
    var atStart = !has || state.current <= 0;
    var atEnd = !has || state.current >= steps.length - 1;
    $('btn-first').disabled = atStart;
    $('btn-prev').disabled = atStart;
    $('btn-next').disabled = atEnd;
    $('btn-last').disabled = atEnd;
    $('btn-play').disabled = !has;
    $('btn-play').textContent = state.playing ? '⏸ 暂停' : '▶ 播放';
  }

  function isValid(json) {
    return json && typeof json === 'object' &&
      Array.isArray(json.items) && Array.isArray(json.steps);
  }

  function loadData(json, source, preserve) {
    if (!isValid(json)) {
      P.toast('数据格式不完整：至少需要 items 与 steps 数组', 'bad');
      return;
    }
    if (!Array.isArray(json.messages)) json.messages = [];
    var sameRound=state.data && state.data.round_id===json.round_id && (state.active || state.source===source);
    if(!sameRound){state.graphItem=null;state.graphMatch=null;state.viewScroll={};state.graphRestoreTop=0;state.resetColumn=true;$('graph-find').value='';state.graphRequest++;}
    var oldSeq = state.data && state.data.steps[state.current] && state.data.steps[state.current].message_seq;
    var atEnd = !state.data || state.current >= state.data.steps.length - 1;
    state.data = json;
    state.source = source || '';
    state.meta = P.buildMeta({ round: { items: json.items } });
    state.current = state.active ? Math.max(0, json.steps.length - 1) : 0;
    if (preserve && !atEnd) {
      var oldIndex = json.steps.findIndex(function (step) { return step.message_seq === oldSeq; });
      if (oldIndex >= 0) state.current = oldIndex;
    }
    if(!preserve || !sameRound)state.focusedKey = null;
    setPlaying(false);
    buildDerived();

    $('empty-hint').classList.add('hidden');
    $('replay-main').classList.remove('hidden');
    renderWhoWhats();
    renderCheck();
    renderAll();
    updateControls();
  }


  function loadDefault(preserve) {
    if (state.inFlight) { state.queuedLoad = true; return; }
    state.inFlight = true;
    var requestId = ++state.requestId;
    $('active-refresh').disabled = true;
    var base = P.stripSlash($('api').value) || P.resolveApiBase();
    P.get(base + '/api/replay/view').catch(function (err) {
      if (err.status !== 404) throw err;
      return Promise.all([P.get(base + '/api/config'), P.get(base + '/api/display?since=0')]).then(function (parts) {
        var cfg = parts[0].config || parts[0];
        var live = parts[1];
        if (live.board && live.board.round_id && live.board.round_id !== cfg.round.round_id) throw new Error('轮次正在切换，请刷新');
        return {round_id:cfg.round.round_id, title:cfg.round.title, version:live.version,
          items:cfg.round.items || [], messages:[], steps:[], board:live.board,
          who_whats:live.who_whats, latest_only:true};
      });
    }).then(function (json) {
      if (requestId !== state.requestId || !state.active) return;
      if (state.expectedRound && json.round_id !== state.expectedRound) {
        state.queuedLoad = true;
        return;
      }
      loadData(json, '当前激活轮次', preserve);
      $('replay-notice').textContent = json.latest_only ? '已显示当前激活排位。逐步历史需要服务更新后刷新。' : (json.steps.length ? '选择左侧消息查看该步排位；默认显示最新状态。' : '当前激活轮次暂无复盘消息。');
      $('replay-notice').className = 'hint replay-notice';
    }).catch(function (err) {
      if (requestId !== state.requestId || !state.active) return;
      $('replay-notice').textContent = '读取当前轮次失败：' + P.errorText(err);
      $('replay-notice').className = 'banner show bad';
    }).finally(function () {
      state.inFlight = false;
      $('active-refresh').disabled = false;
      if (state.queuedLoad && state.active) { state.queuedLoad = false; loadDefault(true); }
    });
  }

  function bindFileInput() {
    $('file-input').addEventListener('change', function (ev) {
      var file = ev.target.files && ev.target.files[0];
      if (!file) return;
      var reader = new FileReader();
      reader.onload = function () {
        try {
          var imported = JSON.parse(String(reader.result));
          if (!isValid(imported)) throw new Error('需要 items 和 steps 数组');
          state.active = false;
          state.requestId++;
          loadData(imported, file.name);
          $('replay-notice').textContent='正在查看导入文件，完全只读；不会自动切换激活轮次或写入本机消息。';
          $('replay-notice').className='hint replay-notice';
          P.toast('已载入 ' + file.name, 'ok');
        } catch (e) {
          P.toast('JSON 解析失败：' + (e && e.message ? e.message : e), 'bad');
        }
      };
      reader.onerror = function () { P.toast('文件读取失败', 'bad'); };
      reader.readAsText(file, 'utf-8');
      ev.target.value = '';
    });
  }

  function switchView(view) {
    var content=document.querySelector('.replay-content');
    state.viewScroll[state.view]={top:content.scrollTop,left:content.scrollLeft};
    var position=state.viewScroll[view] || {top:0,left:0};
    if(view==='mermaid')state.graphRestoreTop=position.top;
    state.view = view;
    $('tab-table').classList.toggle('active', view === 'table');
    $('tab-column').classList.toggle('active', view === 'column');
    $('tab-mermaid').classList.toggle('active', view === 'mermaid');
    renderRight();
    content.scrollTop=position.top;content.scrollLeft=position.left;
  }

  function init() {
    $('api').value = P.resolveApiBase();
    $('api').addEventListener('change', function () {
      state.active = true;
      state.expectedRound = '';
      state.workflowStamp = '';
      state.requestId++;
      loadDefault();
    });
    tableEl = $('table-view');
    boardRenderer = P.createBoardRenderer(tableEl, { persistent: true });
    P.boardLayout.bindControls($('board-width'));
    P.boardLayout.onMode(function () {
      if (!state.data || state.view !== 'column') return;
      var root = $('column-view'), scroll = root.querySelector('.col-wrap');
      var left = scroll ? scroll.scrollLeft : 0;
      renderColumn();
      scroll = root.querySelector('.col-wrap');
      if (scroll) scroll.scrollLeft = left;
    });

    tableEl.addEventListener('click', function (ev) {
      var cell = ev.target && ev.target.closest ? ev.target.closest('.cell') : null;
      if (!cell || !cell.__k) return;
      onCellClick(cell.__k);
    });

    $('btn-first').addEventListener('click', function () { setPlaying(false); goToStep(0); });
    $('process-message').addEventListener('click',function(){
      if(!state.active)return;var step=state.data.steps[state.current],message=step && msgBySeq(step.message_seq);
      if(message && message.record_id && state.data.round_id)P.navigate(P.pageHref('display.html',{source:'local',record:message.record_id,message_round:state.data.round_id}));
    });
    $('btn-prev').addEventListener('click', function () { setPlaying(false); goToStep(state.current - 1); });
    $('btn-next').addEventListener('click', function () { setPlaying(false); goToStep(state.current + 1); });
    $('btn-last').addEventListener('click', function () {
      setPlaying(false);
      goToStep((state.data && state.data.steps ? state.data.steps.length : 0) - 1);
    });
    $('btn-play').addEventListener('click', function () { setPlaying(!state.playing); });
    $('speed').addEventListener('change', function (e) { state.speed = parseFloat(e.target.value) || 1; });

    $('tab-table').addEventListener('click', function () { switchView('table'); });
    $('tab-column').addEventListener('click', function () { switchView('column'); });
    $('tab-mermaid').addEventListener('click', function () { switchView('mermaid'); });
    $('graph-item').addEventListener('change',function(){state.graphItem=this.value;state.graphMatch=null;renderMermaid();});
    $('graph-locate').addEventListener('click',locateGraph);
    $('graph-find').addEventListener('keydown',function(event){if(event.key==='Enter'){event.preventDefault();locateGraph();}});
    $('graph-fit').addEventListener('click',function(){fitGraph(true);});
    $('graph-actual').addEventListener('click',function(){fitGraph(false);});
    window.addEventListener('resize',function(){if(state.view==='mermaid')$('mermaid-view').querySelectorAll('.graph-card').forEach(sizeGraph);});

    $('ww-toggle').addEventListener('click', function () {
      state.wwOpen = !state.wwOpen;
      renderWhoWhats();
    });

    document.addEventListener('keydown', function (e) {
      if(e.target && e.target.closest && e.target.closest('input,select,textarea,button,a,summary,[contenteditable="true"]'))return;
      if (!state.data) return;
      var steps = state.data.steps || [];
      if (e.key === 'ArrowRight') { e.preventDefault(); setPlaying(false); goToStep(state.current + 1); }
      else if (e.key === 'ArrowLeft') { e.preventDefault(); setPlaying(false); goToStep(state.current - 1); }
      else if (e.key === ' ') { e.preventDefault(); setPlaying(!state.playing); }
      else if (e.key === 'Home') { e.preventDefault(); setPlaying(false); goToStep(0); }
      else if (e.key === 'End') { e.preventDefault(); setPlaying(false); goToStep(steps.length - 1); }
    });

    bindFileInput();
    $('active-refresh').addEventListener('click', function () { state.active = true;state.expectedRound='';state.workflowStamp='';state.requestId++;$('process-message').disabled=true;setPlaying(false);loadDefault(); });
    if (window.PAIGU_SHELL) window.PAIGU_SHELL.onWorkflow(function (wf) {
      if (!wf || !state.active) return;
      var stamp = [wf.round_id, wf.revision, wf.version, wf.messages,wf.view_epoch,wf.view_revision].join('|');
      state.expectedRound = wf.round_id;
      if (stamp !== state.workflowStamp || !state.data) {
        var changedRound = state.data && state.data.round_id !== wf.round_id;
        state.workflowStamp = stamp;
        if (changedRound) {
          state.requestId++;
          setPlaying(false);
          state.data = null;
          $('replay-main').classList.add('hidden');
          $('who-whats-panel').classList.add('hidden');
        }
        loadDefault(!changedRound);
      }
    });
    updateControls();
    loadDefault();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
