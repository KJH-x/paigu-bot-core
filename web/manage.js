(function (global) {
  'use strict';

  function pad2(n) { return (n < 10 ? '0' : '') + n; }

  function msToLocalInput(ms) {
    if (ms == null || ms === '') return '';
    var d = new Date(Number(ms));
    if (isNaN(d.getTime())) return '';
    return d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate()) +
      'T' + pad2(d.getHours()) + ':' + pad2(d.getMinutes());
  }

  function localInputToMs(s) {
    if (!s) return null;
    var d = new Date(s);
    return isNaN(d.getTime()) ? null : d.getTime();
  }

  /** 词组切分：按空白 / 逗号 / 顿号 / 斜杠 / 分号 / 竖线切分，trim + 去重。 */
  function splitTokens(s) {
    var parts = String(s == null ? '' : s).split(/[\s,，、/／;；|]+/);
    var out = [];
    var seen = {};
    for (var i = 0; i < parts.length; i++) {
      var t = parts[i].trim();
      if (!t || seen[t]) continue;
      seen[t] = true;
      out.push(t);
    }
    return out;
  }

  /** 逗号/换行列表（gateway 白名单、priority_users 等）。 */
  function splitList(s) {
    return String(s == null ? '' : s).split(/[,，\n]/)
      .map(function (x) { return x.trim(); })
      .filter(function (x) { return x !== ''; });
  }

  function joinList(a) {
    return (Array.isArray(a) ? a : []).join(', ');
  }

  function asStrArray(v) {
    if (v == null) return [];
    if (Array.isArray(v)) {
      return v.map(function (x) { return String(x == null ? '' : x).trim(); })
        .filter(function (x) { return x !== ''; });
    }
    return splitTokens(v);
  }

  function dedupe(arr) {
    var out = [];
    var seen = {};
    for (var i = 0; i < (arr || []).length; i++) {
      var t = String(arr[i] == null ? '' : arr[i]).trim();
      if (!t || seen[t]) continue;
      seen[t] = true;
      out.push(t);
    }
    return out;
  }

  function el(tag, cls, text) {
    var n = global.document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  global.PAIGU_MANAGE = {
    pad2: pad2,
    msToLocalInput: msToLocalInput,
    localInputToMs: localInputToMs,
    splitTokens: splitTokens,
    splitList: splitList,
    joinList: joinList,
    asStrArray: asStrArray,
    dedupe: dedupe,
    el: el
  };
})(window);
