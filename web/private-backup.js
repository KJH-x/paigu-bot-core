(function () {
  'use strict';
  const P = window.PAIGU, LIMIT = 64 * 1024 * 1024;
  P.mountPrivateBackups = function (options) {
    const $ = id => document.getElementById(id);
    let selectedFile = null, preflight = null, generation = 0, job = null;
    const controls = ['snap-export', 'snap-out', 'snap-file', 'snap-path', 'snap-import', 'snap-apply', 'snap-download'];
    const report = (text, bad) => options.report(text, !!bad);
    function scope() {
      const value = options.context();
      if (!value) return null;
      const url = new URL(P.resolveApiBase(), location.href);
      if (url.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(url.hostname) || url.username || url.password || url.pathname !== '/' || url.search || url.hash) return null;
      return {base: url.origin, round: value.round_id, revision: value.revision};
    }
    function current(context) {
      const value = scope();
      if (!value || value.base !== context.base || value.round !== context.round || value.revision !== context.revision) return false;
      const shell = window.PAIGU_SHELL && window.PAIGU_SHELL.getState();
      return !shell || !shell.ok || !shell.workflow || (shell.workflow.round_id === context.round && shell.workflow.revision === context.revision);
    }
    function source() {
      return selectedFile ? {kind: 'file', file: selectedFile, generation} : {kind: 'path', path: $('snap-path').value.trim(), generation};
    }
    function sameSource(a, b) {
      return a.generation === b.generation && a.kind === b.kind && (a.kind === 'file' ? a.file === b.file : a.path === b.path);
    }
    function validProof() {
      return preflight && preflight.data.applicable && current(preflight.context) && sameSource(preflight.source, source());
    }
    function refresh() {
      let context = null;
      try { context = scope(); } catch (_) {}
      const usable = !!context && current(context), busy = !!job;
      controls.forEach(id => { $(id).disabled = busy || !usable; });
      $('snap-download').disabled = busy || !usable || !$('snap-private-ack').checked;
      $('snap-download-cancel').hidden = !job || job.kind !== 'download';
      $('snap-import').disabled = busy || !usable || (!selectedFile && !$('snap-path').value.trim()) || !!(selectedFile && selectedFile.size > LIMIT);
      $('snap-apply').disabled = busy || !usable || !validProof();
      $('snap-context').textContent = usable ? '只备份已保存数据；页面草稿不包含在内。浏览器文件上限64MiB，更大备份请使用处理主机路径。' : '请连接本机HTTP处理API并重新读取当前轮次和配置；私有文件不会发送到外站。';
    }
    function invalidate() { generation++; preflight = null; refresh(); }
    function cancelDownload(message) {
      if (!job || job.kind !== 'download') return;
      clearTimeout(job.timer); job.controller.abort(); job = null;
      $('snap-download-info').textContent = message || '已取消交付文件；不会下载迟到结果。'; refresh();
    }
    function begin(kind) {
      const context = scope();
      if (job || !context || !current(context)) throw new Error('上下文已变化，请重新读取后再操作；未提交或下载。');
      const value = {kind, context, controller: new AbortController()}; job = value;
      value.timer = setTimeout(() => {
        if (job !== value) return;
        value.controller.abort(); job = null; preflight = null;
        const text = kind === 'apply' ? '应用请求超时，可能已经提交；请重新读取确认，不要反复应用。' : '请求超时，未采用迟到结果；可重新读取后重试。';
        $(kind === 'download' ? 'snap-download-info' : kind === 'export' ? 'snap-export-info' : 'snap-import-result').textContent = text;
        report(text, true); refresh();
      }, 60000);
      refresh(); return value;
    }
    function end(value) { clearTimeout(value.timer); if (job === value) { job = null; refresh(); } }
    async function responseError(response) {
      let data = null; try { data = await response.json(); } catch (_) {}
      const error = new Error(response.status === 404 || response.status === 405 ? '当前服务尚未提供浏览器备份入口；可使用处理主机路径。' : 'HTTP ' + response.status);
      error.status = response.status; error.data = data; return error;
    }
    async function fetchRaw(value, route, body) {
      const response = await fetch(value.context.base + route, {method: 'POST', cache: 'no-store',
        headers: {'Content-Type': 'application/json', Accept: 'application/json'}, body, signal: value.controller.signal});
      if (job !== value || value.controller.signal.aborted) throw new Error('请求已中断，未采用迟到结果。');
      if (!response.ok) throw await responseError(response);
      return response;
    }
    async function recheck(value) {
      if (job !== value || value.controller.signal.aborted || !current(value.context)) throw new Error('API、轮次或配置已变化，旧上下文结果未交付；请重新读取。');
      const latest = await P.get(value.context.base + '/api/config', {signal: value.controller.signal});
      if (latest.config.round.round_id !== value.context.round || latest.revision !== value.context.revision || job !== value || !current(value.context)) throw new Error('轮次或配置已变化，未交付旧上下文备份；请重新读取。');
    }
    async function download() {
      if (!$('snap-private-ack').checked) { report('请先确认私有备份不可公开分享。', true); return; }
      let value;
      try {
        value = begin('download'); $('snap-download-info').textContent = '正在生成完整私有备份…';
        const response = await fetchRaw(value, '/api/snapshot/download', JSON.stringify({round_id: value.context.round, config_revision: value.context.revision, private_ack: true}));
        const disposition = response.headers.get('Content-Disposition') || '';
        const name = /filename="(paigu-private-[a-f0-9]{64}\.snapshot\.json)"/.exec(disposition);
        if (!name || !(response.headers.get('Content-Type') || '').toLowerCase().startsWith('application/json')) throw new Error('响应不是可验证的备份附件，未下载。');
        if (Number(response.headers.get('Content-Length')) > LIMIT) throw new Error('备份超过64MiB，请保存到处理主机；不会截断文件。');
        const blob = await response.blob();
        if (blob.size > LIMIT) throw new Error('备份超过64MiB，请保存到处理主机；不会截断文件。');
        await recheck(value);
        if (!$('snap-private-ack').checked) throw new Error('隐私确认已撤回，未下载。');
        const url = URL.createObjectURL(blob), link = document.createElement('a');
        link.href = url; link.download = name[1]; link.hidden = true; document.body.appendChild(link);
        try { link.click(); } finally { link.remove(); setTimeout(() => URL.revokeObjectURL(url), 5000); }
        $('snap-download-info').textContent = '已交给浏览器保存 ' + name[1] + '。恢复时选择该文件，再预检；不可公开分享。'; report('');
      } catch (error) {
        if (value && job === value) {
          $('snap-download-info').textContent = error.name === 'AbortError' ? '下载已中断，未交付文件。' : '未下载：' + P.errorText(error);
          report($('snap-download-info').textContent, true);
        } else if (!value) report(P.errorText(error), true);
      } finally { if (value) end(value); }
    }
    async function exportPath() {
      let value;
      try {
        value = begin('export'); $('snap-export-info').textContent = '正在保存到处理主机…';
        const body = {format: 'file', round_id: value.context.round, config_revision: value.context.revision};
        if ($('snap-out').value.trim()) body.out_dir = $('snap-out').value.trim();
        const result = await P.post(value.context.base + '/api/snapshot/export', body, {signal: value.controller.signal});
        await recheck(value);
        selectedFile = null; $('snap-file').value = ''; $('snap-file-info').textContent = '';
        $('snap-path').value = result.path || ''; generation++; preflight = null;
        $('snap-export-info').textContent = '已保存到处理主机：' + result.path + '；可用下方路径预检。'; report('');
      } catch (error) {
        if (value && job !== value) return;
        $('snap-export-info').textContent = '保存未完成或上下文已变化：' + P.errorText(error); report($('snap-export-info').textContent, true);
      } finally { if (value) end(value); }
    }
    async function importSource(apply, protectedDrafts) {
      if (apply && !protectedDrafts) return P.protectAction(() => importSource(true, true)).catch(error => report(P.errorText(error), true));
      let value;
      const picked = source(), proof = preflight;
      try {
        if (apply && !validProof()) throw new Error('请先在当前上下文重新预检当前文件或路径。');
        if ((picked.kind === 'file' && picked.file.size > LIMIT) || (picked.kind === 'path' && !picked.path)) throw new Error('请选择不超过64MiB的文件，或填写处理主机上的备份路径。');
        value = begin(apply ? 'apply' : 'preflight');
        $('snap-import-result').textContent = apply ? '正在应用；如中断，请重新读取确认结果，不要假定已撤销。' : '正在校验文件与当前轮次；尚未应用。';
        let result;
        const revision = apply ? proof.data.config_revision : value.context.revision;
        if (picked.kind === 'file') {
          const query = new URLSearchParams({round_id: value.context.round, revision: String(revision), apply: String(!!apply)});
          if (apply) query.set('preflight_token', proof.data.preflight_token);
          const response = await fetchRaw(value, '/api/snapshot/import-file?' + query, picked.file);
          result = await response.json();
        } else {
          const body = {path: picked.path, apply: !!apply, round_id: value.context.round, revision};
          if (apply) body.preflight_token = proof.data.preflight_token;
          result = await P.post(value.context.base + '/api/snapshot/import', body, {signal: value.controller.signal});
        }
        if (job !== value || value.controller.signal.aborted || !sameSource(picked, source()) || !current(value.context)) throw new Error(apply ? '应用结果返回时上下文已变化，请重新读取确认；未把旧结果套到新轮次。' : '文件、路径或上下文已变化，旧预检未采用。');
        preflight = result.applied ? null : {data: result, source: picked, context: value.context};
        $('snap-import-result').textContent = result.applied ? '已应用并重算。原完整日志保留在处理主机：' + result.backup_path :
          '备份轮次：' + result.manifest.round_id + ' → 当前轮：' + result.target_round_id + '；消息 ' + result.current_messages + ' → ' + result.import_messages + '。' + result.effect + (result.applicable ? '' : '请手动激活匹配轮次后重新预检。');
        report(''); P.toast(result.applied ? '备份已应用' : '预检完成，尚未应用', 'ok');
        if (result.applied) {
          end(value); await options.onApplied();
          if (window.PAIGU_SHELL) window.PAIGU_SHELL.refresh();
        }
      } catch (error) {
        if (value && job !== value) return;
        preflight = null;
        $('snap-import-result').textContent = (apply ? '应用未完成或结果待确认，请重新读取：' : '预检未完成：') + P.errorText(error);
        report($('snap-import-result').textContent, true);
      } finally { if (value) end(value); }
    }
    $('snap-download').addEventListener('click', download);
    $('snap-download-cancel').addEventListener('click', () => cancelDownload());
    $('snap-private-ack').addEventListener('change', () => { if (!$('snap-private-ack').checked) cancelDownload('隐私确认已撤回，未交付文件。'); refresh(); });
    $('snap-export').addEventListener('click', exportPath);
    $('snap-import').addEventListener('click', () => importSource(false));
    $('snap-apply').addEventListener('click', () => importSource(true));
    $('snap-file').addEventListener('change', () => {
      selectedFile = $('snap-file').files[0] || null; $('snap-path').value = '';
      $('snap-file-info').textContent = !selectedFile ? '' : selectedFile.size > LIMIT ? '文件超过64MiB，请使用处理主机路径；未上传。' : '已选择 ' + selectedFile.name + '，仅点击预检时才上传到本机处理API。';
      invalidate();
    });
    $('snap-path').addEventListener('input', () => { selectedFile = null; $('snap-file').value = ''; $('snap-file-info').textContent = ''; invalidate(); });
    if (window.PAIGU_SHELL) window.PAIGU_SHELL.onWorkflow(() => {
      if (job && job.kind === 'download' && !current(job.context)) cancelDownload('API、轮次或配置已变化，未下载迟到结果。');
      if (preflight && !current(preflight.context)) { preflight = null; $('snap-import-result').textContent = '轮次或配置已变化，请重新预检。'; }
      refresh();
    });
    refresh();
    return {refresh, reset: () => { generation++; preflight = null; cancelDownload('重新读取已中止旧下载交付；请重新确认后发起。'); refresh(); }};
  };
})();
