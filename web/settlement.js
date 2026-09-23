(function () {
  'use strict';

  var P = window.PAIGU;
  function $(id) { return document.getElementById(id); }

  var state = {
    revision: null,
    config: defaultConfig(),
    table: { packages: [] },
    result: null,
    plans: {},
    evalTimer: null
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
          variant_id: p.variant_id != null ? p.variant_id : null,
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
        entry.item_id = v.trim(); scheduleEval();
      }, 'item-ids'));
      row.appendChild(inputEl('text', entry.variant_id || '', 'variant_id(可空)', function (v) {
        entry.variant_id = v.trim() ? v.trim() : null; scheduleEval();
      }, 'variant-ids'));
      row.appendChild(selectEl([['AdjustBy', '原价加减'], ['SetFinal', '直设调后价']], entry.mode, function (v) {
        entry.mode = v; scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.value ? entry.value / 100 : '', '金额(元)', function (v) {
        entry.value = toCents(v); scheduleEval();
      }));
      row.appendChild(buttonEl('删除', 'small danger', function () {
        state.config.pricing.splice(i, 1); renderConfig(); scheduleEval();
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
        entry.rule_id = v.trim(); scheduleEval();
      }));
      row.appendChild(selectEl([['Threshold', '满减'], ['WholeOrder', '全单']], entry.kind, function (v) {
        entry.kind = v; scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.amount ? entry.amount / 100 : '', '减额(元)', function (v) {
        entry.amount = toCents(v); scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.threshold != null ? entry.threshold / 100 : '', '门槛(元,满减)', function (v) {
        entry.threshold = v.trim() === '' ? null : toCents(v); scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.ratio_ppm != null ? entry.ratio_ppm / 10000 : '', '折扣率(%)', function (v) {
        entry.ratio_ppm = v.trim() === '' ? null : Math.round(parseFloat(v) * 10000); scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.shares, '份数n(-1=全部)', function (v) {
        entry.shares = v.trim() === '' ? -1 : intOf(v, -1); scheduleEval();
      }));
      row.appendChild(buttonEl('删除', 'small danger', function () {
        state.config.discounts.splice(i, 1); renderConfig(); scheduleEval();
      }));
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
        entry.tier_id = v.trim(); scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.threshold ? entry.threshold / 100 : '', '门槛(元)', function (v) {
        entry.threshold = toCents(v); scheduleEval();
      }));
      row.appendChild(inputEl('text', entry.gift_name, '特典名', function (v) {
        entry.gift_name = v; scheduleEval();
      }));
      row.appendChild(inputEl('number', entry.unit_price ? entry.unit_price / 100 : '', '特典价(元)', function (v) {
        entry.unit_price = toCents(v); scheduleEval();
      }));
      var claimedInput = inputEl('number', entry.claimed, '认购数 claimed', function (v) {
        entry.claimed = Math.max(0, intOf(v, 0));
        delete entry.__claimedMissing;
        refreshClaimedWarn();
        scheduleEval();
      });
      claimedInput.min = '0';
      claimedInput.step = '1';
      row.appendChild(claimedInput);
      row.appendChild(buttonEl('删除', 'small danger', function () {
        state.config.gift_tiers.splice(i, 1); renderConfig(); scheduleEval();
      }));
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
        var raw = e.dataTransfer.getData('text/plain');
        if (!raw) return;
        var parts = raw.split(':');
        moveLine(intOf(parts[0], -1), intOf(parts[1], -1), pi);
      });

      var head = el('div', 'pkg-head');
      head.appendChild(el('span', 'pkg-id', pkg.package_id));
      head.appendChild(el('span', 'spacer'));
      head.appendChild(buttonEl('删包', 'small danger', function () {
        state.table.packages.splice(pi, 1); renderOrderTable(); evaluate(true);
      }));
      card.appendChild(head);

      if (!pkg.lines.length) card.appendChild(el('div', 'pkg-empty', '（空包，可从其它包拖入）'));
      pkg.lines.forEach(function (line, li) {
        card.appendChild(lineRow(pkg, line, pi, li));
      });

      var addWrap = el('div', 'pkg-add-line');
      addWrap.appendChild(buttonEl('＋ 加行', 'small', function () {
        pkg.lines.push({ item_id: '', variant_id: null, qty: 1, unit_price_cents: 0, is_gift: false });
        renderOrderTable();
      }));
      card.appendChild(addWrap);
      host.appendChild(card);
    });
  }

  function lineRow(pkg, line, pi, li) {
    var row = el('div', 'line' + (line.is_gift ? ' gift' : ''));
    var handle = el('span', 'line-handle', '⠿');
    handle.draggable = true;
    handle.title = '拖到其它包';
    handle.addEventListener('dragstart', function (e) {
      e.dataTransfer.setData('text/plain', pi + ':' + li);
      e.dataTransfer.effectAllowed = 'move';
    });
    row.appendChild(handle);

    row.appendChild(inputEl('text', line.item_id, 'item_id', function (v) {
      line.item_id = v.trim(); scheduleEval();
    }, 'item-ids'));
    row.appendChild(inputEl('text', line.variant_id || '', 'variant', function (v) {
      line.variant_id = v.trim() ? v.trim() : null; scheduleEval();
    }, 'variant-ids'));

    var qty = inputEl('number', line.qty, '数量', function (v) {
      line.qty = Math.max(0, intOf(v, 0));
      row.querySelector('.line-total').textContent = totalText(line);
      scheduleEval();
    });
    qty.min = '0';
    row.appendChild(qty);

    row.appendChild(inputEl('number', line.unit_price_cents ? line.unit_price_cents / 100 : '', '单价(元)', function (v) {
      line.unit_price_cents = toCents(v);
      row.querySelector('.line-total').textContent = totalText(line);
      scheduleEval();
    }));

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
      pkg.lines.splice(li, 1); renderOrderTable(); evaluate(true);
    }));
    return row;
  }

  function moveLine(si, li, dst) {
    if (si < 0 || li < 0 || si === dst) return;
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
        && (l.variant_id || null) === (line.variant_id || null)
        && !!l.is_gift === !!line.is_gift
        && l.unit_price_cents === line.unit_price_cents) { found = l; break; }
    }
    if (found) found.qty += line.qty;
    else target.lines.push(line);
    renderOrderTable();
    evaluate(true);
  }

  function addPackage() {
    state.table.packages.push({ package_id: '包' + (state.table.packages.length + 1), lines: [] });
    renderOrderTable();
  }

  function scheduleEval() {
    evaluate(false);
  }

  function evaluate(immediate) {
    if (state.evalTimer) { clearTimeout(state.evalTimer); state.evalTimer = null; }
    var run = function () {
      if (!state.table.packages || !state.table.packages.length) {
        state.result = null; renderResult(); return;
      }
      setConn('busy', '试算中…');
      P.post('/api/settlement/evaluate', { order_table: state.table, config: requestConfig() })
        .then(function (res) {
          state.result = res;
          setConn('ok', '已试算');
          hideBanner();
          renderResult();
        })
        .catch(function (err) {
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
    tbl.innerHTML = '<thead><tr><th>下单包</th><th>item_id</th><th>variant</th><th>数量</th><th>标价(元)</th><th>标价合计(元)</th><th>减均(元)</th><th>减均后合计(元)</th><th>减均后单价(元)</th></tr></thead>';
    var tbody = document.createElement('tbody');
    lines.forEach(function (l) {
      var tr = document.createElement('tr');
      [
        l.package_id, l.item_id, l.variant_id || '—', String(l.qty),
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
    ['gift_max', 'discount_max'].forEach(function (s) {
      state.plans[s] = { loading: true };
      renderPlan(s);
      P.post('/api/settlement/plan', { strategy: s, order_table: state.table, config: requestConfig() })
        .then(function (res) {
          state.plans[s] = { data: res };
          renderPlan(s);
        })
        .catch(function (err) {
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
      state.table = P.deepClone(best);
      renderOrderTable();
      evaluate(true);
      P.toast('已应用 ' + (res.strategy || s) + ' 方案', 'ok');
    });
    apply.classList.add('apply');
    body.appendChild(apply);
    body.appendChild(el('div', 'note', '方案由 T5 planner（DFS/回溯）产出；包名为自动生成，可在表内继续拖拽调整。'));
  }

  function loadSample() {
    state.config = normalizeConfig({
      pricing: [{ item_id: 'item_a', variant_id: null, mode: 'AdjustBy', value: 1000 }],
      discounts: [{ rule_id: 'd_whole', kind: 'WholeOrder', amount: 0, threshold: null, ratio_ppm: 100000, shares: -1 }],
      scope_mode: 'ExcludeGift',
      gift_tiers: [
        { tier_id: 't0', threshold: 0, gift_name: '特典-基础', unit_price: 1000, claimed: 3 },
        { tier_id: 't300', threshold: 30000, gift_name: '特典-满300', unit_price: 3000, claimed: 2 },
        { tier_id: 't500', threshold: 50000, gift_name: '特典-满500', unit_price: 5000, claimed: 1 }
      ],
      reduce_average: { include_gift_price: false }
    });
    state.table = {
      packages: [
        { package_id: '示例用户01#p1', lines: [{ item_id: 'item_a', variant_id: 'v1', qty: 1, unit_price_cents: 50100, is_gift: false }] },
        { package_id: '示例用户02#p2', lines: [{ item_id: 'item_a', variant_id: 'v2', qty: 1, unit_price_cents: 30000, is_gift: false }] },
        { package_id: '示例用户03#p3', lines: [{ item_id: 'item_b', variant_id: null, qty: 2, unit_price_cents: 15000, is_gift: false }] }
      ]
    };
    renderConfig();
    renderOrderTable();
    evaluate(true);
    P.toast('已载入占位示例', 'ok');
  }

  function fromAllocation() {
    P.post('/api/settlement/from-allocation', {}).then(function (res) {
      if (res.empty || !res.order_table || !res.order_table.packages.length) {
        showBanner('当前排位为空，无法生成下单表。可先用模拟器产生排位，或载入占位示例。', true);
        return;
      }
      state.table = res.order_table;
      renderOrderTable();
      evaluate(true);
      P.toast('已从当前排位生成 ' + state.table.packages.length + ' 个包（单价默认 0，可在表内编辑）', 'ok');
    }).catch(function (err) {
      showBanner('生成失败：' + P.errorText(err), true);
    });
  }

  function loadConfig() {
    setConn('busy', '载入中…');
    return P.get('/api/settlement/config').then(function (res) {
      state.revision = res.revision;
      state.config = normalizeConfig(res.config);
      renderConfig();
      setRev();
      setConn('ok', '配置已载入');
      hideBanner();
    }).catch(function (err) {
      setConn('bad', '配置载入失败');
      showBanner('结算配置载入失败：' + P.errorText(err), true);
    });
  }

  function loadMeta() {
    P.get('/api/config').then(function (res) {
      var cfg = res && res.config ? res.config : res;
      var items = (cfg && cfg.round && cfg.round.items) || [];
      var itemList = $('item-ids');
      var varList = $('variant-ids');
      itemList.innerHTML = '';
      varList.innerHTML = '';
      items.forEach(function (it) {
        var o = document.createElement('option');
        o.value = it.item_id;
        if (it.name) o.label = it.name;
        itemList.appendChild(o);
        (it.variants || []).forEach(function (v) {
          var ov = document.createElement('option');
          ov.value = v.variant_id;
          if (v.name) ov.label = v.name;
          varList.appendChild(ov);
        });
      });
    }).catch(function () { /* 可选数据 */ });
  }

  function saveConfig() {
    if (state.revision == null) {
      showBanner('尚未载入配置，无法保存。', true);
      return;
    }
    P.put('/api/settlement/config', { config: requestConfig(), revision: state.revision }).then(function (res) {
      state.revision = res.revision;
      state.config = normalizeConfig(res.config);
      renderConfig();
      setRev();
      setConn('ok', '已保存');
      hideBanner();
      P.toast('结算配置已保存', 'ok');
    }).catch(function (err) {
      if (err.status === 409) {
        var rev = err.data && err.data.revision != null ? err.data.revision : '?';
        showBanner('配置冲突：revision 不匹配（服务器 ' + rev + '），请点「重新载入」后再保存。', true);
      } else {
        showBanner('保存失败：' + P.errorText(err), true);
      }
    });
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

    $('load').addEventListener('click', loadConfig);
    $('save').addEventListener('click', saveConfig);
    $('evaluate').addEventListener('click', function () { evaluate(true); });
    $('load-sample').addEventListener('click', loadSample);
    $('load-allocation').addEventListener('click', fromAllocation);
    $('pkg-add').addEventListener('click', addPackage);
    $('run-plans').addEventListener('click', runPlans);
    $('price-add').addEventListener('click', function () {
      state.config.pricing.push({ item_id: '', variant_id: null, mode: $('price-mode-global').value, value: 0 });
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
      state.config.scope_mode = e.target.value; scheduleEval();
    });
    $('reduce-include').addEventListener('change', function (e) {
      state.config.reduce_average.include_gift_price = e.target.checked; scheduleEval();
    });

    renderConfig();
    renderOrderTable();
    loadMeta();
    loadConfig();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
