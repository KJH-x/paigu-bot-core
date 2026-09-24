(function () {
  'use strict';

  var P = window.PAIGU;
  function $(id) { return document.getElementById(id); }

  function start() {
    var api = $('api');
    if (api) api.value = P.resolveApiBase();

    function render(wf) {
      var host = $('admin-summary');
      if (!host) return;
      if (!wf) { host.textContent = '工作流不可用（API 未连接）。'; return; }
      var gw = wf.gateway || {};
      var rows = [
        ['round_id', wf.round_id],
        ['title', wf.title],
        ['phase', wf.phase == null ? '阶段未配置' : (wf.phase_label || wf.phase)],
        ['claims', wf.claims],
        ['version', wf.version],
        ['clients', gw.clients],
        ['locked', wf.locked],
        ['settlement_configured', wf.settlement_configured]
      ];
      var html = '';
      for (var i = 0; i < rows.length; i++) {
        html += '<div class="k">' + P.esc(rows[i][0]) + '</div><div class="v">' +
          P.esc(rows[i][1] == null ? '—' : rows[i][1]) + '</div>';
      }
      host.innerHTML = html;
    }

    if (window.PAIGU_SHELL && window.PAIGU_SHELL.onWorkflow) window.PAIGU_SHELL.onWorkflow(render);
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
