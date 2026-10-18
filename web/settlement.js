(function () {
  'use strict';

  var P = window.PAIGU;
  function $(id) { return document.getElementById(id); }

  var state = {
    revision: null,
    roundId: '',
    config: defaultConfig(),
    table: { packages: [] },
    result: null,
    plans: {},
    evalTimer: null,
    draftRevision: 0,
    orderNumbers: {},
    selectedStrategy: null,
    locked: false,
    draftTimer: null,
    draftSaveChain: Promise.resolve(),
    generation: 0,
    draftEdits: 0,
    savedEdits: 0,
    draftConflict: false,
    sourceToken: '',
    currentSourceToken: '',
    sourceConflict: false,
    needsReview: false,
    resultStale: false,
    issues: [],
    loading: true,
    loadFailed: false,
    saving: false,
    reviewing: false,
    lockPending: false,
    evaluation: 0,
    suppressSave: false,
    catalog: []
  };

  function defaultConfig() {
    return {
      pricing: [],
      discounts: [],
      scope_mode: 'ExcludeGift',
      gift_tiers: [],
      reduce_average: { include_gift_price: false }
    };
  }

  function intOf(v, d) {
    var n = parseInt(v, 10);
    return isNaN(n) ? d : n;
  }

  function toCents(v) {
    var n = parseFloat(v);
    return isNaN(n) ? 0 : Math.round(n * 100);
  }

  function money(cents) {
    return (Math.round(cents || 0) / 100).toFixed(2);
  }

  function sum(arr, key) {
    return (arr || []).reduce(function (acc, item) { return acc + (item[key] || 0); }, 0);
  }

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text != null) node.textContent = text;
    return node;
  }

  function inputEl(type, value, placeholder, onInput, listId) {
    var inp = document.createElement('input');
    inp.type = type;
    inp.value = value == null ? '' : value;
    if (placeholder) inp.placeholder = placeholder;
    if (listId) inp.setAttribute('list', listId);
    if (type === 'number') inp.step = 'any';
    inp.addEventListener('input', function () { onInput(inp.value); });
    return inp;
  }

  function selectEl(options, value, onChange) {
    var sel = document.createElement('select');
    options.forEach(function (o) {
      var op = document.createElement('option');
      op.value = o[0];
      op.textContent = o[1];
      sel.appendChild(op);
    });
    sel.value = value;
    sel.addEventListener('change', function () { onChange(sel.value); });
    return sel;
  }

  function labelField(control, title) {
    var label = el('label', 'cfg-field');
    label.appendChild(el('span', '', title));
    control.setAttribute('aria-label', title);
    label.appendChild(control);
    return label;
  }

  function labelConfigRow(row, titles) {
    var controls = Array.prototype.slice.call(row.children).filter(function (node) { return node.tagName === 'INPUT' || node.tagName === 'SELECT'; });
    controls.forEach(function (control, index) {
      var next = control.nextSibling;
      row.insertBefore(labelField(control, titles[index]), next);
    });
  }

  function buttonEl(text, cls, onClick) {
    var btn = document.createElement('button');
    btn.textContent = text;
    if (cls) btn.className = cls;
    btn.addEventListener('click', onClick);
    return btn;
  }

  function setConn(kind, text) {
    var badge = $('conn-badge');
    badge.className = 'status-badge' + (kind ? ' ' + kind : '');
    badge.innerHTML = '<span class="dot"></span>' + P.esc(text);
  }

  function setRev() {
    $('rev-badge').textContent = 'revision ' + (state.revision == null ? '—' : state.revision);
  }

  function showBanner(msg, bad) {
    var b = $('banner');
    b.textContent = msg;
    b.className = 'banner show' + (bad ? ' bad' : '');
  }

  function hideBanner() { $('banner').className = 'banner'; }

  function normalizeConfig(raw) {
    var c = raw || {};
    return {
      pricing: Array.isArray(c.pricing) ? c.pricing.map(function (p) {
        return {
          item_id: p.item_id || '',
          subitem_id: p.subitem_id != null ? p.subitem_id : null,
          mode: p.mode === 'SetFinal' ? 'SetFinal' : 'AdjustBy',
          value: intOf(p.value, 0)
        };
      }) : [],
      discounts: Array.isArray(c.discounts) ? c.discounts.map(function (d) {
        return {
          rule_id: d.rule_id || '',
          kind: d.kind === 'WholeOrder' ? 'WholeOrder' : 'Threshold',
          amount: intOf(d.amount, 0),
          threshold: d.threshold != null ? intOf(d.threshold, 0) : null,
          ratio_ppm: d.ratio_ppm != null ? intOf(d.ratio_ppm, 0) : null,
          shares: d.shares != null ? intOf(d.shares, -1) : -1
        };
      }) : [],
      scope_mode: c.scope_mode === 'IncludeGift' ? 'IncludeGift' : 'ExcludeGift',
      gift_tiers: Array.isArray(c.gift_tiers) ? c.gift_tiers.map(function (t) {
        var entry = {
          tier_id: t.tier_id || '',
          threshold: intOf(t.threshold, 0),
          gift_name: t.gift_name || '',
          unit_price: intOf(t.unit_price, 0),
          claimed: intOf(t.claimed, 0)
        };
        if (t.claimed == null) entry.__claimedMissing = true;
        return entry;
      }) : [],
      reduce_average: { include_gift_price: !!(c.reduce_average && c.reduce_average.include_gift_price) }
    };
  }

  function requestConfig() {
    var c = P.deepClone(state.config);
    (c.gift_tiers || []).forEach(function (t) { delete t.__claimedMissing; });
    return c;
  }

  function renderConfig() {
    renderPricing();
    renderDiscounts();
    renderTiers();
    $('scope-mode').value = state.config.scope_mode;
    $('reduce-include').checked = state.config.reduce_average.include_gift_price;
  }

  function renderPricing() {
    var host = $('pricing-list');
    host.innerHTML = '';
    var list = state.config.pricing;
    if (!list.length) { host.appendChild(el('div', 'hint', '暂无调价条目')); return; }
    list.forEach(function (entry, i) {
      var row = el('div', 'cfg-row');
      row.appendChild(inputEl('text', entry.item_id, 'item_id', function (v) {
        entry.item_id = v.trim(); scheduleRuleEval();
      }, 'item-ids'));
      row.appendChild(inputEl('text', entry.subitem_id || '', 'subitem_id(可空)', function (v) {
        entry.subitem_id = v.trim() ? v.trim() : null; scheduleRuleEval();
      }, 'subitem-ids'));
      row.appendChild(selectEl([['AdjustBy', '原价加减'], ['SetFinal', '直设调后价']], entry.mode, function (v) {
        entry.mode = v; scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.value ? entry.value / 100 : '', '金额(元)', function (v) {
        entry.value = toCents(v); scheduleRuleEval();
      }));
      row.appendChild(buttonEl('删除', 'small danger', function () {
        state.config.pricing.splice(i, 1); renderConfig(); scheduleRuleEval();
      }));
      host.appendChild(row);
    });
  }

  function renderDiscounts() {
    var host = $('discount-list');
    host.innerHTML = '';
    var list = state.config.discounts;
    if (!list.length) { host.appendChild(el('div', 'hint', '暂无折扣条目')); return; }
    list.forEach(function (entry, i) {
      var row = el('div', 'cfg-row');
      row.appendChild(inputEl('text', entry.rule_id, 'rule_id', function (v) {
        entry.rule_id = v.trim(); scheduleRuleEval();
      }));
      row.appendChild(selectEl([['Threshold', '满减'], ['WholeOrder', '全单']], entry.kind, function (v) {
        entry.kind = v; scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.amount ? entry.amount / 100 : '', '减额(元)', function (v) {
        entry.amount = toCents(v); scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.threshold != null ? entry.threshold / 100 : '', '门槛(元,满减)', function (v) {
        entry.threshold = v.trim() === '' ? null : toCents(v); scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.ratio_ppm != null ? entry.ratio_ppm / 10000 : '', '折扣率(%)', function (v) {
        entry.ratio_ppm = v.trim() === '' ? null : Math.round(parseFloat(v) * 10000); scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.shares, '份数n(-1=全部)', function (v) {
        entry.shares = v.trim() === '' ? -1 : intOf(v, -1); scheduleRuleEval();
      }));
      row.appendChild(buttonEl('删除', 'small danger', function () {
        state.config.discounts.splice(i, 1); renderConfig(); scheduleRuleEval();
      }));
      labelConfigRow(row, ['规则名称', '优惠方式', '减额(元)', '门槛(元)', '折扣率（%）', '适用包数（-1 为全部）']);
      host.appendChild(row);
    });
  }

  function missingClaimedTiers() {
    return (state.config.gift_tiers || []).filter(function (t) { return t.__claimedMissing; });
  }

  function refreshClaimedWarn() {
    var host = $('tier-list');
    var warn = host.querySelector('.tier-claimed-warn');
    var missing = missingClaimedTiers();
    if (!warn && missing.length) {
      warn = el('div', 'banner tier-claimed-warn');
      host.insertBefore(warn, host.firstChild);
    }
    if (!warn) return;
    if (missing.length) {
      warn.className = 'banner show tier-claimed-warn';
      warn.textContent = '注意：以下特典档位缺少 claimed（认购数），已按 0 处理，将不授予特典：'
        + missing.map(function (t) { return t.tier_id || '(未命名)'; }).join('、')
        + '。请补填认购数后保存或试算。';
    } else {
      warn.className = 'banner tier-claimed-warn';
      warn.textContent = '';
    }
  }

  function renderTiers() {
    var host = $('tier-list');
    host.innerHTML = '';
    var list = state.config.gift_tiers;
    if (!list.length) { host.appendChild(el('div', 'hint', '暂无特典档位')); return; }
    list.forEach(function (entry, i) {
      var row = el('div', 'cfg-row');
      row.appendChild(inputEl('text', entry.tier_id, 'tier_id', function (v) {
        entry.tier_id = v.trim(); scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.threshold ? entry.threshold / 100 : '', '门槛(元)', function (v) {
        entry.threshold = toCents(v); scheduleRuleEval();
      }));
      row.appendChild(inputEl('text', entry.gift_name, '特典名', function (v) {
        entry.gift_name = v; scheduleRuleEval();
      }));
      row.appendChild(inputEl('number', entry.unit_price ? entry.unit_price / 100 : '', '特典价(元)', function (v) {
        entry.unit_price = toCents(v); scheduleRuleEval();
      }));
      var claimedInput = inputEl('number', entry.claimed, '认购数 claimed', function (v) {
        entry.claimed = Math.max(0, intOf(v, 0));
        delete entry.__claimedMissing;
        refreshClaimedWarn();
        scheduleRuleEval();
      });
      claimedInput.min = '0';
      claimedInput.step = '1';
      row.appendChild(claimedInput);
      row.appendChild(buttonEl('删除', 'small danger', function () {
        state.config.gift_tiers.splice(i, 1); renderConfig(); scheduleRuleEval();
      }));
      labelConfigRow(row, ['档位名称', '门槛(元)', '特典名称', '单价(元)', '认购数量']);
      host.appendChild(row);
    });
    refreshClaimedWarn();
  }

  function totalText(line) {
    return (line.qty * (line.unit_price_cents || 0) / 100).toFixed(2) + ' 元';
  }

  function renderOrderTable() {
    var host = $('order-table');
    host.innerHTML = '';
    var pkgs = state.table.packages || [];
    $('order-empty').classList.toggle('hidden', pkgs.length > 0);
    pkgs.forEach(function (pkg, pi) {
      var card = el('div', 'pkg');
      card.addEventListener('dragover', function (e) {
        if(state.locked)return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        card.classList.add('drop');
      });
      card.addEventListener('dragleave', function (e) {
        if (!card.contains(e.relatedTarget)) card.classList.remove('drop');
      });
      card.addEventListener('drop', function (e) {
        e.preventDefault();
        card.classList.remove('drop');
        if(state.locked)return;
        var raw = e.dataTransfer.getData('text/plain');
        if (!raw) return;
        var parts = raw.split(':');
        moveLine(intOf(parts[0], -1), intOf(parts[1], -1), pi);
      });

      var head = el('div', 'pkg-head');
      card.dataset.packageIndex=pi;card.tabIndex=-1;
      var name = inputEl('text', pkg.package_id, '包名称', function(value){
        var old=pkg.package_id;pkg.package_id=value;
        if(!Object.prototype.hasOwnProperty.call(state.orderNumbers,value) && state.table.packages.filter(function(p){return p.package_id===old;}).length===0)state.orderNumbers[value]=state.orderNumbers[old] || '';
        renderOrderNumbers();scheduleDraftSave();
      });
      name.className='pkg-id';name.dataset.field='package_name';name.setAttribute('aria-label','第 '+(pi+1)+' 包名称');head.appendChild(name);
      head.appendChild(el('span', 'spacer'));
      head.appendChild(buttonEl('删包', 'small danger', function () {
        state.table.packages.splice(pi, 1); renderOrderTable(); evaluate(true); scheduleDraftSave();
      }));
      card.appendChild(head);

      if (!pkg.lines.length) card.appendChild(el('div', 'pkg-empty', '（空包，可从其它包拖入）'));
      pkg.lines.forEach(function (line, li) {
        card.appendChild(lineRow(pkg, line, pi, li));
      });

      var addWrap = el('div', 'pkg-add-line');
      addWrap.appendChild(buttonEl('＋ 加行', 'small', function () {
        pkg.lines.push({ item_id: '', subitem_id: null, qty: 1, unit_price_cents: 0, is_gift: false });
        renderOrderTable();
        scheduleDraftSave();
      }));
      card.appendChild(addWrap);
      host.appendChild(card);
    });
    renderOrderNumbers();
    applyLockState();
  }

  function renderOrderNumbers() {
    var host = $('order-numbers');
    if (!host) return;
    host.innerHTML = '';
    (state.table.packages || []).forEach(function (pkg,pi) {
      if(!(pkg.lines || []).some(function(line){return line.qty>0;}))return;
      var row = el('div', 'row');
      row.dataset.packageIndex=pi;
      row.appendChild(el('span', 'mono', pkg.package_id));
      var number=inputEl('text', state.orderNumbers[pkg.package_id] || '', '填写订单号', function (value) {
        state.orderNumbers[pkg.package_id] = value.trim();
        scheduleDraftSave();
      });
      number.dataset.field='order_number';number.setAttribute('aria-label',pkg.package_id+' 的订单号');row.appendChild(number);
      host.appendChild(row);
    });
  }

  function applyLockState() {
    var frozen=state.locked || state.loading || state.loadFailed || state.reviewing || state.lockPending;
    var host = $('order');
    if (host) {
      Array.prototype.forEach.call(host.querySelectorAll('input,button,select'), function (control) {
        if (control.id !== 'unlock-order' && !control.closest('#order-issues')) control.disabled = frozen;
      });
    }
    Array.prototype.forEach.call(document.querySelectorAll('.line-handle'),function(handle){handle.draggable=!frozen;handle.setAttribute('aria-disabled',String(!!frozen));});
    var configHost = $('settle');
    if (configHost) {
      Array.prototype.forEach.call(configHost.querySelectorAll('input,button,select'), function (control) {
        control.disabled = frozen;
      });
    }
    $('lock-order').classList.toggle('hidden', !!state.locked);
    $('unlock-order').classList.toggle('hidden', !state.locked);
    $('save').disabled = frozen;
    $('draft-status').textContent = state.locked ? '已锁定 · revision ' + state.draftRevision :
      (state.draftConflict ? '保存冲突 · 本地草稿保留' : state.saving ? '保存中…' : state.sourceConflict ? '来源已变化 · 输入保留' : state.draftEdits!==state.savedEdits ? '有未保存改动' : '已保存 · revision ' + state.draftRevision);
    $('lock-order').disabled=frozen || state.needsReview || state.sourceConflict || state.draftConflict;
    $('review-order').disabled=frozen || state.draftConflict;
    renderReview();
  }

  function lineRow(pkg, line, pi, li) {
    var row = el('div', 'line' + (line.is_gift ? ' gift' : ''));
    row.dataset.lineIndex=li;
    var handle = el('span', 'line-handle', '⠿');
    handle.draggable = !state.locked;
    handle.title = '拖到其它包';
    handle.addEventListener('dragstart', function (e) {
      if(state.locked){e.preventDefault();return;}
      e.dataTransfer.setData('text/plain', pi + ':' + li);
      e.dataTransfer.effectAllowed = 'move';
    });
    row.appendChild(handle);

    var itemOptions = state.catalog.map(function (item) { return [item.item_id, item.name || item.item_id]; });
    if (!itemOptions.some(function (option) { return option[0] === line.item_id; })) itemOptions.unshift([line.item_id, line.item_id]);
    row.appendChild(labelField(selectEl(itemOptions, line.item_id, function (v) {
      line.item_id = v; line.subitem_id = null; renderOrderTable(); scheduleEval();
    }), '商品'));
    var item = state.catalog.find(function (entry) { return entry.item_id === line.item_id; });
    var subitemOptions = [['', '单领 / 无款式']].concat(((item && item.subitems) || []).map(function (subitem) { return [subitem.subitem_id, subitem.name || subitem.subitem_id]; }));
    if (line.subitem_id && !subitemOptions.some(function (option) { return option[0] === line.subitem_id; })) subitemOptions.push([line.subitem_id, line.subitem_id]);
    row.appendChild(labelField(selectEl(subitemOptions, line.subitem_id || '', function (v) {
      line.subitem_id = v || null; scheduleEval();
    }), '款式'));

    var qty = inputEl('number', line.qty, '数量', function (v) {
      line.qty = Math.max(0, intOf(v, 0));
      row.querySelector('.line-total').textContent = totalText(line);
      scheduleEval();
    });
    qty.min = '0';
    qty.step = '1';
    row.appendChild(labelField(qty, '数量'));

    row.appendChild(labelField(inputEl('number', line.unit_price_cents ? line.unit_price_cents / 100 : '', '单价(元)', function (v) {
      line.unit_price_cents = toCents(v);
      row.querySelector('.line-total').textContent = totalText(line);
      scheduleEval();
    }), '单价(元)'));

    var giftLabel = el('label', 'gift-toggle');
    var gift = document.createElement('input');
    gift.type = 'checkbox';
    gift.checked = !!line.is_gift;
    gift.addEventListener('change', function () {
      line.is_gift = gift.checked;
      row.classList.toggle('gift', gift.checked);
      scheduleEval();
    });
    giftLabel.appendChild(gift);
    giftLabel.appendChild(document.createTextNode('特典'));
    row.appendChild(giftLabel);

    row.appendChild(el('span', 'line-total', totalText(line)));
    row.appendChild(buttonEl('删', 'small danger', function () {
      pkg.lines.splice(li, 1); renderOrderTable(); evaluate(true); scheduleDraftSave();
    }));
    var move=el('div','line-move');
    var choices=[['','选择目标包']].concat(state.table.packages.map(function(p,i){return [String(i),p.package_id];}).filter(function(p){return p[0]!==String(pi);}));
    var target=selectEl(choices,'',function(){});target.setAttribute('aria-label','移动 '+(item && item.name || line.item_id)+' 到包');
    move.appendChild(target);move.appendChild(buttonEl('移动','small',function(){if(target.value==='')return;moveLine(pi,li,intOf(target.value,-1));}));
    row.appendChild(move);
    var fields=row.querySelectorAll('.cfg-field');['item','subitem','quantity','price'].forEach(function(field,i){var control=fields[i] && fields[i].querySelector('input,select');if(control)control.dataset.field=field;});
    return row;
  }

  function applyReview(view) {
    state.statusRequest=(state.statusRequest || 0)+1;
    state.currentSourceToken=view.source_token || state.currentSourceToken;
    state.needsReview=!!view.needs_review;state.resultStale=!!view.result_stale;
    if(Array.isArray(view.issues))state.issues=view.issues;
  }

  function renderReview() {
    if(!$('review-status'))return;
    var changed=state.needsReview || state.sourceConflict;
    $('order-review').classList.toggle('needs-review',changed);
    $('review-status').textContent=state.loading ? '正在核对来源…' : state.loadFailed ? '草稿读取失败 · 尚不能写入' : state.reviewing ? '正在保留订单并重新核对…' : changed ? '上游数据已变化 · 需要重新核对' : '订单来源已核对'+(state.draftEdits===state.savedEdits && state.issues.length ? ' · 还有 '+state.issues.length+' 项订单问题' : '');
    $('result-source').textContent=state.resultStale || changed ? '旧来源试算 · 待复核' : state.draftEdits!==state.savedEdits ? '当前输入 · 尚未保存' : '';
    $('review-order').hidden=!changed;
    var issues=state.draftEdits===state.savedEdits ? state.issues.slice() : [];
    if(state.draftEdits!==state.savedEdits){
      var numbers={},prices={};
      state.table.packages.forEach(function(pkg,pi){
        if(!pkg.lines.some(function(l){return l.qty>0;}))return;
        var number=(state.orderNumbers[pkg.package_id] || '').trim();
        if(!number || numbers[number])issues.push({code:'order_number',message:pkg.package_id+(number?'：订单号重复':'：请填写订单号'),package_index:pi,field:'order_number'});
        numbers[number]=true;
        pkg.lines.forEach(function(line,li){
          if(!line.qty || line.is_gift)return;
          var key=JSON.stringify([line.item_id,line.subitem_id]),item=state.catalog.find(function(i){return i.item_id===line.item_id;});
          if(line.unit_price_cents<=0 || (prices[key]!=null && prices[key]!==line.unit_price_cents))issues.push({code:'price',message:pkg.package_id+'：'+(item && item.name || line.item_id)+' 标价需大于零且各包一致',package_index:pi,line_index:li,field:'price'});
          prices[key]=line.unit_price_cents;
        });
      });
    }
    var host=$('order-issues'),signature=JSON.stringify(issues);
    if(host.dataset.signature===signature)return;
    host.dataset.signature=signature;host.replaceChildren();
    issues.forEach(function(issue){var btn=buttonEl('↳ '+issue.message,'small',function(){locateIssue(issue);});btn.dataset.issue=issue.code;host.appendChild(btn);});
  }

  function locateIssue(issue) {
    var target;
    if(issue.field==='allocation')target=$('load-allocation');
    else if(issue.field==='order_number')target=$('order-numbers').querySelector('[data-package-index="'+issue.package_index+'"] input');
    else {
      var pkg=$('order-table').querySelector('[data-package-index="'+issue.package_index+'"]');
      var row=issue.line_index==null ? pkg : pkg && pkg.querySelector('[data-line-index="'+issue.line_index+'"]');
      target=row && (row.querySelector('[data-field="'+issue.field+'"]') || row.querySelector('input,select') || row);
    }
    if(!target)return;
    target.scrollIntoView({block:'center',behavior:'instant'});target.focus({preventScroll:true});
    target.classList.add('order-issue-target');setTimeout(function(){target.classList.remove('order-issue-target');},1800);
  }

  function refreshStatus() {
    if(state.loading || !state.roundId || state.statusBusy)return Promise.resolve();
    var generation=state.generation,round=state.roundId,request=state.statusRequest=(state.statusRequest || 0)+1;
    state.statusBusy=true;
    return P.get('/api/settlement/status').then(function(view){
      if(generation!==state.generation || request!==state.statusRequest)return;
      if(view.round_id!==round){state.draftConflict=true;showBanner('当前激活轮次已切换；本页输入保留，请重新读取正确轮次后再操作。',true);applyLockState();return;}
      if(view.revision<state.draftRevision)return;
      if(view.revision>state.draftRevision && !state.saving)state.draftConflict=true;
      if(view.source_token!==state.sourceToken)state.sourceConflict=true;
      state.locked=!!view.locked;applyReview(view);applyLockState();
    }).catch(function(){
      if(generation===state.generation)$('review-status').textContent='来源状态暂不可用 · 锁定时仍会严格校验';
    }).finally(function(){state.statusBusy=false;});
  }

  function recheckOrder() {
    if(state.locked || state.loading || state.reviewing || state.draftConflict)return;
    if(state.rulesBaseline && JSON.stringify(requestConfig())!==state.rulesBaseline){showBanner('请先保存或放弃结算规则草稿，再重新核对订单。当前订单保留。',true);$('save').focus();return;}
    var generation=state.generation,round=state.roundId;
    state.reviewing=true;state.evaluation++;
    if(state.evalTimer){clearTimeout(state.evalTimer);state.evalTimer=null;}
    applyLockState();
    state.draftSaveChain.catch(function(){}).then(function(){return Promise.all([P.get('/api/config'),P.get('/api/settlement/status')]);}).then(function(values){
      var cfg=values[0],view=values[1];
      if(generation!==state.generation || cfg.config.round.round_id!==round || view.round_id!==round)throw new Error('轮次已变化，未复核其他轮次');
      if(cfg.revision!==view.config_revision)throw new Error('读取期间配置又有变化，请重试；订单未替换');
      if(view.revision!==state.draftRevision)throw new Error('服务器订单已被其他操作修改；输入保留，请先核对版本冲突');
      state.config=normalizeConfig(cfg.config.settlement);state.catalog=cfg.config.round.items || [];state.revision=cfg.revision;
      state.serverRules=P.deepClone(cfg.config.settlement);state.savedRules=P.deepClone(state.config);state.rulesBaseline=JSON.stringify(requestConfig());
      renderConfig();setRev();applyReview(view);
      return saveDraft(true);
    }).then(function(){if(generation===state.generation){hideBanner();P.toast('已按当前来源重新核对；排包、标价与订单号均保留','ok');}}).catch(function(err){if(generation===state.generation)showBanner('重新核对未完成：'+P.errorText(err),true);}).finally(function(){if(generation===state.generation){state.reviewing=false;applyLockState();}});
  }

  function moveLine(si, li, dst) {
    if (state.locked || state.loading || state.reviewing || state.lockPending || si < 0 || li < 0 || dst<0 || si === dst) return;
    var pkgs = state.table.packages;
    if (si >= pkgs.length || dst >= pkgs.length) return;
    var pkg = pkgs[si];
    if (li >= pkg.lines.length) return;
    var line = pkg.lines.splice(li, 1)[0];
    var target = pkgs[dst];
    var found = null;
    for (var k = 0; k < target.lines.length; k++) {
      var l = target.lines[k];
      if (l.item_id === line.item_id
        && (l.subitem_id || null) === (line.subitem_id || null)
        && !!l.is_gift === !!line.is_gift
        && l.unit_price_cents === line.unit_price_cents) { found = l; break; }
    }
    if (found) found.qty += line.qty;
    else target.lines.push(line);
    renderOrderTable();
    evaluate(true);
    scheduleDraftSave();
    var destination=$('order-table').querySelector('[data-package-index="'+dst+'"]');if(destination)destination.focus({preventScroll:true});
  }

  function addPackage() {
    state.table.packages.push({ package_id: '包' + (state.table.packages.length + 1), lines: [] });
    renderOrderTable();
    scheduleDraftSave();
  }

  function scheduleRuleEval() {
    evaluate(false);renderReview();
  }

  function scheduleEval() {
    evaluate(false);
    scheduleDraftSave();
  }

  function scheduleDraftSave() {
    if (state.suppressSave || state.locked) return;
    state.draftEdits++;
    if (state.draftTimer) clearTimeout(state.draftTimer);
    $('draft-status').textContent = '等待保存…';
    renderReview();
    state.draftTimer = setTimeout(function () { saveDraft().catch(function(){}); }, 500);
  }

  function saveDraft(ackReview) {
    if (state.locked) return Promise.resolve(null);
    if (state.loading || state.loadFailed) return Promise.reject(new Error('草稿尚未成功读取，未写入订单'));
    if (state.draftTimer) { clearTimeout(state.draftTimer); state.draftTimer = null; }
    var generation=state.generation, round=state.roundId, base=P.resolveApiBase(), edit=state.draftEdits, knownSource=state.currentSourceToken;
    var payload={round_id:round,source_token:ackReview ? state.currentSourceToken : state.sourceToken,ack_review:!!ackReview,order_table:P.deepClone(state.table),selected_strategy:state.selectedStrategy,order_numbers:P.deepClone(state.orderNumbers)};
    state.draftSaveChain = state.draftSaveChain.catch(function () { return null; }).then(function () {
      if(generation!==state.generation || round!==state.roundId || base!==P.resolveApiBase()) throw new Error('旧页面保存已取消，未写入其他轮次或实例');
      if(state.draftConflict) throw new Error('请先处理保存冲突，本地草稿仍保留');
      if(state.sourceConflict && !ackReview)throw new Error('来源已变化，请先重新核对；输入保留');
      state.saving=true;
      $('draft-status').textContent = '保存中…';
      payload.revision=state.draftRevision;
      return P.put(base+'/api/settlement/draft', payload).then(function (res) {
        if(generation!==state.generation || round!==state.roundId) return res;
        var draft = res.draft || {};
        if(draft.round_id!==round) throw new Error('服务返回的轮次不匹配，未套用结果');
        state.draftRevision = draft.revision || state.draftRevision;
        state.savedEdits=edit;
        var changedDuringSave=state.currentSourceToken!==knownSource && state.currentSourceToken!==payload.source_token,currentSource=state.currentSourceToken;
        state.sourceToken=payload.source_token;state.sourceConflict=changedDuringSave;
        applyReview(res);
        if(changedDuringSave){state.currentSourceToken=currentSource;state.needsReview=true;state.resultStale=true;}
        if(edit===state.draftEdits)state.result = draft.last_result || state.result;
        state.locked = !!draft.locked;
        renderResult();
        applyLockState();
        return res;
      }).catch(function (err) {
        if(generation!==state.generation)throw err;
        if(err.status===409 && err.data && err.data.error==='settlement_source_changed'){
          state.sourceConflict=true;state.needsReview=true;state.resultStale=true;
        }else if(err.status===409)state.draftConflict=true;
        $('draft-status').textContent = '保存失败';
        // A conflict must not replace the administrator's unsaved order table.
        showBanner('草稿保存失败：' + P.errorText(err), true);
        throw err;
      }).finally(function(){if(generation===state.generation){state.saving=false;applyLockState();}});
    });
    return state.draftSaveChain;
  }

  function applyDraft(draft,view) {
    if(draft.round_id && draft.round_id!==state.roundId)throw new Error('轮次已切换，请重新载入；未套用另一轮草稿');
    state.draftRevision = draft.revision || 0;
    state.table = draft.order_table || { packages: [] };
    state.result = draft.last_result || null;
    state.selectedStrategy = draft.selected_strategy || null;
    state.orderNumbers = draft.order_numbers || {};
    state.locked = !!draft.locked;
    state.savedEdits=state.draftEdits=0;state.draftConflict=false;state.sourceConflict=false;
    state.sourceToken=(view && view.source_token) || '';applyReview(view || {});
    renderOrderTable();
    renderResult();
    applyLockState();
  }

  function loadDraft() {
    state.suppressSave = true;
    var generation=state.generation;
    return P.get('/api/settlement/draft').then(function (res) {
      if(generation!==state.generation)return;
      applyDraft(res.draft || {},res);
      state.loadFailed=false;
      $('draft-status').textContent = state.locked ? '已锁定 · revision ' + state.draftRevision : '已保存 · revision ' + state.draftRevision;
    }).catch(function (err) {
      if(generation!==state.generation)return;
      state.loadFailed=true;
      showBanner('结算草稿载入失败：' + P.errorText(err), true);
    }).then(function () { if(generation===state.generation){state.suppressSave = false;state.loading=false;applyLockState();} });
  }

  function lockOrder() {
    if(state.lockPending || state.locked || state.loading)return;
    if(state.rulesBaseline && JSON.stringify(requestConfig())!==state.rulesBaseline){showBanner('请先保存或放弃结算规则草稿，再锁定订单。',true);$('save').focus();return;}
    var generation=state.generation,round=state.roundId;
    state.lockPending=true;state.evaluation++;applyLockState();
    saveDraft().then(function () {
      if(generation!==state.generation || round!==state.roundId)throw new Error('页面上下文已改变，未锁定其他轮次');
      return P.post('/api/settlement/lock', { round_id:round,revision: state.draftRevision,source_token:state.sourceToken });
    }).then(function (res) {
      if(generation!==state.generation)return;
      applyDraft(res.draft || {},res);
      hideBanner();
      P.toast('订单已锁定并持久化', 'ok');
    }).catch(function (err) {
      if(generation!==state.generation)return;
      if(err.data && err.data.issues){state.issues=err.data.issues;renderReview();}
      refreshStatus();
      showBanner('锁定失败：' + P.errorText(err), true);
    }).finally(function(){if(generation===state.generation){state.lockPending=false;applyLockState();}});
  }

  function unlockOrder() {
    if (!window.confirm('确认解锁？解锁后消息、配置和下单表可继续修改。')) return;
    var generation=state.generation;
    P.post('/api/settlement/unlock', { round_id:state.roundId,revision: state.draftRevision }).then(function (res) {
      if(generation!==state.generation)return;
      applyDraft(res.draft || {},res);
      hideBanner();
      P.toast('已解锁', 'ok');
    }).catch(function (err) { showBanner('解锁失败：' + P.errorText(err), true); });
  }

  function evaluate(immediate) {
    if (state.evalTimer) { clearTimeout(state.evalTimer); state.evalTimer = null; }
    var generation=state.generation, evaluation=++state.evaluation;
    var run = function () {
      if (!state.table.packages || !state.table.packages.length) {
        state.result = null; renderResult(); return;
      }
      setConn('busy', '试算中…');
      P.post('/api/settlement/evaluate', { order_table: state.table, config: requestConfig() })
        .then(function (res) {
          if(generation!==state.generation || evaluation!==state.evaluation)return;
          state.result = res;
          setConn('ok', '已试算');
          hideBanner();
          renderResult();
        })
        .catch(function (err) {
          if(generation!==state.generation || evaluation!==state.evaluation)return;
          setConn('bad', '试算失败');
          showBanner('试算失败：' + P.errorText(err), true);
        });
    };
    if (immediate) run();
    else state.evalTimer = setTimeout(run, 300);
  }

  function renderResult() {
    var r = state.result;
    var has = !!r;
    $('result-empty').classList.toggle('hidden', has);
    $('result-body').classList.toggle('hidden', !has);
    if (!has) { $('result-summary').textContent = ''; return; }

    $('result-summary').textContent = '包数 ' + r.packages.length;

    var totals = $('totals');
    totals.innerHTML = '';
    totals.appendChild(stat('折前总额', money(sum(r.packages, 'gross_cents'))));
    totals.appendChild(stat('折扣合计', money(r.discount_total)));
    totals.appendChild(stat('C 无折扣商品总价', money(r.list_total_cents)));
    totals.appendChild(stat('B 实付价合计', money(r.paid_total_cents)));
    totals.appendChild(stat('G 已授予特典价', money(r.gift_valuation_total)));
    totals.appendChild(stat('D 总优惠额', money(r.reduce_average_total)));
    var grand = stat('应付总额', money(r.grand_total));
    grand.classList.add('ok');
    totals.appendChild(grand);

    var body = $('result-packages');
    body.innerHTML = '';
    r.packages.forEach(function (p) {
      var tr = document.createElement('tr');
      var cells = [
        p.package_id, money(p.gross_cents), money(p.adjusted_cents), money(p.discount_cents),
        money(p.reduce_average_cents), money(p.payable_cents), String(p.gift_count), money(p.gift_value_cents)
      ];
      cells.forEach(function (v, idx) {
        var td = document.createElement('td');
        td.textContent = v;
        if (idx >= 1) td.className = 'mono';
        tr.appendChild(td);
      });
      body.appendChild(tr);
    });

    var gifts = $('gift-list');
    gifts.innerHTML = '';
    if (!r.gift_list.length) {
      gifts.appendChild(el('div', 'empty', '无特典命中'));
    } else {
      var tbl = document.createElement('table');
      tbl.className = 'gift-list-table';
      var thead = document.createElement('thead');
      thead.innerHTML = '<tr><th>下单包</th><th>档位</th><th>特典名</th><th>数量</th><th>单价(元)</th></tr>';
      tbl.appendChild(thead);
      var tbody = document.createElement('tbody');
      r.gift_list.forEach(function (g) {
        var tr = document.createElement('tr');
        [g.package_id, g.tier_id, g.gift_name, String(g.quantity), money(g.unit_price_cents)].forEach(function (v, i) {
          var td = document.createElement('td');
          td.textContent = v;
          if (i === 4) td.className = 'mono';
          tr.appendChild(td);
        });
        tbody.appendChild(tr);
      });
      tbl.appendChild(tbody);
      gifts.appendChild(tbl);
    }

    renderLineSettlement(r.lines);

    var checks = validateResult(r);
    var messages = (r.warnings || []).slice().concat(checks);
    var warn = $('warnings');
    if (checks.length) {
      warn.className = 'banner show bad';
      warn.textContent = '校验未通过：' + messages.join('；');
    } else if (messages.length) {
      warn.className = 'banner show';
      warn.textContent = '提示：' + messages.join('；');
    } else {
      warn.className = 'banner show';
      warn.textContent = '校验通过：Σ 减均后商品总价 + G = B，且 D = C − B + G。';
    }
  }

  function validateResult(r) {
    var checks = [];
    if (r.list_total_cents != null && r.paid_total_cents != null) {
      var dExpected = r.list_total_cents - r.paid_total_cents + (r.gift_valuation_total || 0);
      if (dExpected !== (r.reduce_average_total || 0)) {
        checks.push('D 校验失败：C−B+G=' + money(dExpected) + ' ≠ 减均合计 ' + money(r.reduce_average_total || 0));
      }
    }
    var lines = r.lines || [];
    if (lines.length) {
      var lineSum = lines.reduce(function (acc, l) { return acc + (l.final_total_cents || 0); }, 0);
      if (lineSum + (r.gift_valuation_total || 0) !== (r.paid_total_cents || 0)) {
        checks.push('减均校验失败：Σ减均后商品总价(' + money(lineSum) + ')+G(' + money(r.gift_valuation_total || 0) + ') ≠ B(' + money(r.paid_total_cents || 0) + ')');
      }
    }
    return checks;
  }

  function renderLineSettlement(lines) {
    var host = $('line-list');
    if (!host) return;
    host.innerHTML = '';
    lines = lines || [];
    if (!lines.length) { host.appendChild(el('div', 'empty', '无逐行明细')); return; }
    var tbl = document.createElement('table');
    tbl.className = 'gift-list-table';
    tbl.innerHTML = '<thead><tr><th>下单包</th><th>item_id</th><th>subitem</th><th>数量</th><th>标价(元)</th><th>标价合计(元)</th><th>减均(元)</th><th>减均后合计(元)</th><th>减均后单价(元)</th></tr></thead>';
    var tbody = document.createElement('tbody');
    lines.forEach(function (l) {
      var tr = document.createElement('tr');
      [
        l.package_id, l.item_id, l.subitem_id || '—', String(l.qty),
        money(l.unit_price_cents), money(l.total_cents), money(l.reduce_cents),
        money(l.final_total_cents), money(l.final_unit_cents)
      ].forEach(function (v, idx) {
        var td = document.createElement('td');
        td.textContent = v;
        if (idx >= 3) td.className = 'mono';
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });
    tbl.appendChild(tbody);
    host.appendChild(tbl);
  }

  function stat(k, v) {
    var d = el('div', 'stat');
    d.appendChild(document.createTextNode(k + '：'));
    var b = document.createElement('b');
    b.textContent = v;
    d.appendChild(b);
    return d;
  }

  function statLine(k, v) {
    var d = el('div', 'stat-line');
    d.appendChild(el('span', 'k', k));
    d.appendChild(el('span', 'v', v));
    return d;
  }

  function runPlans() {
    if (!state.table.packages || !state.table.packages.length) {
      P.toast('请先生成下单表');
      return;
    }
    var generation=state.generation,edit=state.draftEdits,rules=JSON.stringify(requestConfig());
    ['gift_max', 'discount_max'].forEach(function (s) {
      state.plans[s] = { loading: true };
      renderPlan(s);
      P.post('/api/settlement/plan', { strategy: s, order_table: state.table, config: requestConfig() })
        .then(function (res) {
          if(generation!==state.generation || edit!==state.draftEdits || rules!==JSON.stringify(requestConfig()))return;
          state.plans[s] = { data: res,generation:generation,edit:edit,rules:rules,source:state.sourceToken };
          renderPlan(s);
        })
        .catch(function (err) {
          if(generation!==state.generation)return;
          var msg = err.status === 501
            ? '后端 planner 未就绪（501）：' + ((err.data && err.data.error) || '')
            : P.errorText(err);
          state.plans[s] = { error: msg };
          renderPlan(s);
        });
    });
  }

  function renderPlan(s) {
    var host = $('plan-' + s);
    var body = host.querySelector('.plan-body');
    body.className = 'plan-body';
    body.innerHTML = '';
    var st = state.plans[s];
    if (!st) { body.className = 'plan-body empty'; body.textContent = '未运行。'; return; }
    if (st.loading) { body.className = 'plan-body empty'; body.textContent = '运行中…'; return; }
    if (st.error) { body.appendChild(el('div', 'err', st.error)); return; }

    var res = st.data;
    var score = res.best_score || {};
    var stats = res.stats || {};
    body.appendChild(statLine('策略', res.strategy || s));
    body.appendChild(statLine('最高档命中包数', String(score.highest_tier_hits != null ? score.highest_tier_hits : '-')));
    body.appendChild(statLine('特典命中数', String(score.gift_count != null ? score.gift_count : '-')));
    body.appendChild(statLine('特典折价(元)', money(score.gift_valuation_total || 0)));
    body.appendChild(statLine('折扣合计(元)', money(score.discount_total || 0)));
    body.appendChild(statLine('应付总额(元)', money(score.grand_total || 0)));
    body.appendChild(statLine('搜索评估次数', String(stats.evaluated != null ? stats.evaluated : '-')));
    if (stats.truncated) {
      body.appendChild(statLine('搜索截断', String(stats.truncation_reason || '是')));
    }

    var best = res.best || { packages: [] };
    var apply = buttonEl('应用此方案', 'small primary', function () {
      if(state.locked || st.generation!==state.generation || st.edit!==state.draftEdits || st.rules!==JSON.stringify(requestConfig()) || st.source!==state.sourceToken || state.needsReview || state.sourceConflict){showBanner('方案来源或输入已变化，请重新核对并重新运行策略；当前订单保留。',true);return;}
      if (!res.invariants_ok) { showBanner('方案未通过价格守恒校验，禁止应用。', true); return; }
      state.table = P.deepClone(best);
      state.selectedStrategy = res.strategy || s;
      renderOrderTable();
      evaluate(true);
      scheduleDraftSave();
      P.toast('已应用 ' + (res.strategy || s) + ' 方案', 'ok');
    });
    apply.classList.add('apply');
    body.appendChild(apply);
    body.appendChild(el('div', 'note', '方案由 T5 planner（DFS/回溯）产出；包名为自动生成，可在表内继续拖拽调整。'));
  }

  function fromAllocation() {
    if(state.locked || state.loading)return;
    if(state.table.packages.length && !window.confirm('将重新生成排包，替换当前手工排包；保留已有订单号。是否继续？'))return;
    var generation=state.generation,round=state.roundId,edit=state.draftEdits;
    P.post('/api/settlement/from-allocation', {}).then(function (res) {
      if(generation!==state.generation || round!==state.roundId || edit!==state.draftEdits)throw new Error('等待生成时输入或轮次已变化，当前草稿保留');
      if(res.round_id!==round)throw new Error('当前轮次已切换，未套用其他轮次排位');
      if (res.empty || !res.order_table || !res.order_table.packages.length) {
        showBanner('当前排位为空，未替换订单。请先在排谷页核对当前轮的排位。', true);
        return;
      }
      state.currentSourceToken=state.sourceToken=res.source_token;state.sourceConflict=false;
      state.table = res.order_table;state.selectedStrategy=null;
      renderOrderTable();evaluate(true);scheduleDraftSave();
      P.toast('已从当前排位生成 ' + state.table.packages.length + ' 个包；已有订单号保留，请核对', 'ok');
    }).catch(function (err) { if(generation===state.generation)showBanner('生成失败：' + P.errorText(err), true); });
  }

  function loadConfig() {
    var generation=++state.generation;
    state.loading=true;applyLockState();
    if(state.draftTimer){clearTimeout(state.draftTimer);state.draftTimer=null;}
    if(state.evalTimer){clearTimeout(state.evalTimer);state.evalTimer=null;}
    setConn('busy', '载入中…');
    return P.get('/api/config').then(function (res) {
      res={config:res.config.settlement,revision:res.revision,round_id:res.config.round.round_id,items:res.config.round.items};
      if(generation!==state.generation)return;
      if(state.roundId && state.roundId!==res.round_id){state.table={packages:[]};state.orderNumbers={};state.draftRevision=0;renderOrderTable();}
      state.roundId=res.round_id || '';state.catalog=res.items || [];
      state.revision = res.revision;
      state.config = normalizeConfig(res.config);
      renderConfig();applyLockState();
      state.rulesBaseline=JSON.stringify(requestConfig());state.serverRules=P.deepClone(res.config);state.savedRules=P.deepClone(state.config);
      setRev();
      setConn('ok', '配置已载入');
      hideBanner();
    }).catch(function (err) {
      if(generation!==state.generation)return;
      setConn('bad', '配置载入失败');
      showBanner('结算配置载入失败：' + P.errorText(err), true);
    });
  }

  function saveConfig() {
    if (state.revision == null) {
      showBanner('尚未载入配置，无法保存。', true);
      return Promise.reject(new Error('尚未载入结算规则'));
    }
    var round=state.roundId, rules=requestConfig(), generation=state.generation;
    var controls=Array.from($('settle').querySelectorAll('input,select,textarea,button')).concat([$('save')]), disabled=controls.map(function(n){var value=n.disabled;n.disabled=true;return value;});
    return P.get('/api/config').then(function(latest){
      if(latest.config.round.round_id!==round || generation!==state.generation)throw new Error('轮次已变化，规则草稿保留');
      if(JSON.stringify(latest.config.settlement)!==JSON.stringify(state.serverRules))throw new Error('结算规则已被其他操作修改，草稿保留，请核对后重新读取');
      Object.assign(latest.config.settlement,rules);return P.put('/api/config',{config:latest.config,revision:latest.revision});
    }).then(function (res) {
      if(generation!==state.generation)return;
      state.revision = res.revision;
      state.config = normalizeConfig(res.config.settlement);
      renderConfig();
      state.rulesBaseline=JSON.stringify(requestConfig());state.serverRules=P.deepClone(res.config.settlement);state.savedRules=P.deepClone(state.config);
      setRev();
      setConn('ok', '已保存');
      refreshStatus();
      hideBanner();
      P.toast('结算配置已保存', 'ok');
    }).catch(function (err) {
      if (err.status === 409) {
        var rev = err.data && err.data.revision != null ? err.data.revision : '?';
        showBanner('配置冲突：revision 不匹配（服务器 ' + rev + '），请点「重新载入」后再保存。', true);
      } else {
        showBanner('保存失败：' + P.errorText(err), true);
      }
      throw err;
    }).finally(function(){controls.forEach(function(n,i){if(n.isConnected)n.disabled=disabled[i] || state.locked;});});
  }

  function start() {
    $('api').value = P.resolveApiBase();
    $('api').addEventListener('change', function (e) {
      var v = P.stripSlash(e.target.value);
      var url = new URL(window.location.href);
      if (v && v !== P.DEFAULT_API) url.searchParams.set('api', v);
      else url.searchParams.delete('api');
      window.location.href = url.toString();
    });

    P.registerDraft({id:'settlement-rules',label:'结算规则',isDirty:function(){return !!state.rulesBaseline && JSON.stringify(requestConfig())!==state.rulesBaseline;},save:saveConfig,discard:function(){state.config=P.deepClone(state.savedRules);renderConfig();}});
    P.registerDraft({id:'settlement-order',label:'订单自动保存草稿',isDirty:function(){return state.draftEdits!==state.savedEdits;},save:saveDraft,discard:async function(){
      if(state.draftTimer){clearTimeout(state.draftTimer);state.draftTimer=null;}state.generation++;
      await state.draftSaveChain.catch(function(){});await loadDraft();
      if(state.draftEdits!==state.savedEdits)throw new Error('无法重新读取已保存订单，当前输入保留');
    }});
    $('load').addEventListener('click', function(){P.protectAction(function(){return loadConfig().then(loadDraft);}).catch(function(){});});
    $('save').addEventListener('click', function(){saveConfig().catch(function(){});});
    $('evaluate').addEventListener('click', function () { evaluate(true); });
    $('review-order').addEventListener('click', recheckOrder);
    $('load-allocation').addEventListener('click', fromAllocation);
    $('pkg-add').addEventListener('click', addPackage);
    $('run-plans').addEventListener('click', runPlans);
    $('lock-order').addEventListener('click', lockOrder);
    $('unlock-order').addEventListener('click', unlockOrder);
    $('price-add').addEventListener('click', function () {
      state.config.pricing.push({ item_id: '', subitem_id: null, mode: $('price-mode-global').value, value: 0 });
      renderConfig();
    });
    $('disc-add').addEventListener('click', function () {
      state.config.discounts.push({
        rule_id: 'rule_' + (state.config.discounts.length + 1),
        kind: 'Threshold', amount: 0, threshold: null, ratio_ppm: null, shares: -1
      });
      renderConfig();
    });
    $('tier-add').addEventListener('click', function () {
      state.config.gift_tiers.push({
        tier_id: 'tier_' + (state.config.gift_tiers.length + 1),
        threshold: 0, gift_name: '', unit_price: 0, claimed: 0
      });
      renderConfig();
    });
    $('scope-mode').addEventListener('change', function (e) {
      state.config.scope_mode = e.target.value; scheduleRuleEval();
    });
    $('reduce-include').addEventListener('change', function (e) {
      state.config.reduce_average.include_gift_price = e.target.checked; scheduleRuleEval();
    });

    renderConfig();
    renderOrderTable();
    loadConfig().then(loadDraft);
    if(window.PAIGU_SHELL)PAIGU_SHELL.onWorkflow(function(){refreshStatus();});
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
