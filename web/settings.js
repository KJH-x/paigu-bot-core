(function () {
  'use strict';
  var P=window.PAIGU, M=window.PAIGU_MANAGE, state={config:null,revision:null,baselines:{},server:{},saving:{}};
  function $(id){return document.getElementById(id);}
  function banner(message,bad){var b=$('banner');b.textContent=message || '';b.className='banner'+(message?' show':'')+(bad?' bad':'');}
  function val(id){return $(id).value;}
  function setVal(id,value){$(id).value=value==null?'':value;}
  function setRev(){$('rev-badge').textContent='配置 v'+(state.revision==null?'—':state.revision);}
  function setConn(kind,text){$('conn-badge').textContent=text;$('conn-badge').className='status-badge '+kind;}
  var fields={
    gateway:[
      ['gw-bind','gateway.bind','text'],['gw-heartbeat','gateway.heartbeat_secs','number'],
      ['gw-whitelist','gateway.whitelist_groups','list'],['gw-whitelist-members','gateway.whitelist_members','list'],
      ['gw-test-group','gateway.test_group_id','optional'],['gw-forged','gateway.accept_forged_messages','bool'],
      ['gw-reply','gateway.reply_enabled','bool'],['gw-admincmds','gateway.admin_commands_enabled','bool'],['gw-actions','gateway.allowed_actions','list']
    ],
    understanding:[
      ['llm-enabled','llm.enabled','bool'],['llm-fallback','llm.fallback_to_rules','bool'],
      ['llm-base','llm.base_url','text'],['llm-model','llm.model','text'],['llm-keyenv','llm.api_key_env','text'],
      ['llm-timeout','llm.timeout_secs','number'],['llm-maxtokens','llm.max_tokens','number'],['llm-temp','llm.temperature','number'],
      ['llm-prompt','llm.prompt_template','raw'],['llm-segment','llm.segment_claims','bool'],
      ['merge-enabled','merge.enabled','bool'],['merge-same-sender','merge.same_sender','bool']
    ],
    display:[['dp-refresh','display.refresh_ms','number'],['dp-source','display.data_source','text'],['dp-remote','display.remote_base_url','text']]
  };
  var labels={gateway:'接入设置',understanding:'消息理解设置',display:'展示默认'};
  function getPath(config,path){return path.split('.').reduce(function(v,k){return v && v[k];},config);}
  function setPath(config,path,value){var parts=path.split('.'), group=parts[0];if(!config[group])config[group]={};config[group][parts[1]]=value;}
  function fromConfig(scope,config){var out={};fields[scope].forEach(function(f){out[f[1]]=getPath(config,f[1]);});return out;}
  function fromForm(scope){var out={};fields[scope].forEach(function(f){
    var n=$(f[0]), value=n.value;
    if(f[2]==='bool')value=n.checked;
    else if(f[2]==='number')value=Number(value);
    else if(f[2]==='list')value=M.splitList(value);
    else if(f[2]==='optional')value=value.trim() || null;
    else if(f[2]!=='raw')value=value.trim();
    out[f[1]]=value;
  });return out;}
  function paintScope(scope,config){
    fields[scope].forEach(function(f){var n=$(f[0]), v=getPath(config,f[1]);if(f[2]==='bool')n.checked=!!v;else n.value=f[0]==='llm-temp'?Number(Number(v).toFixed(2)):f[2]==='list'?M.joinList(v):v==null?'':v;});
    state.baselines[scope]=JSON.stringify(fromForm(scope));state.server[scope]=P.deepClone(fromConfig(scope,config));
    $('status-'+scope).textContent='已读取服务器值';
  }
  function dirty(scope){return !!state.config && JSON.stringify(fromForm(scope))!==state.baselines[scope];}
  function restoreScope(scope){paintScope(scope,state.config);}
  function saveScope(scope){
    if(state.saving[scope])return state.saving[scope];
    if(!state.config)return Promise.reject(new Error('配置尚未读取'));
    var container=document.querySelector('[data-settings-scope="'+scope+'"]'), invalid=Array.from(container.querySelectorAll('input,textarea,select')).find(function(n){return !n.checkValidity();});
    if(invalid){invalid.reportValidity();return Promise.reject(new Error('请先修正本分区的输入')); }
    var next=fromForm(scope), base=state.server[scope], round=state.config.round.round_id, nodes=Array.from(container.querySelectorAll('input,textarea,select,button'));
    var disabled=nodes.map(function(n){var d=n.disabled;n.disabled=true;return d;});$('status-'+scope).textContent='正在保存…';
    state.saving[scope]=P.get('/api/config').then(function(res){
      if(res.config.round.round_id!==round)throw new Error('激活轮已变化，当前草稿未提交');
      var latest=fromConfig(scope,res.config), changed=fields[scope].filter(function(f){return JSON.stringify(latest[f[1]])!==JSON.stringify(base[f[1]]);});
      if(changed.length)throw new Error('同分区发生冲突：'+changed.map(function(f){return $(f[0]).closest('label').querySelector('span').textContent;}).join('、')+'。草稿保留，请核对或放弃后重新读取');
      Object.keys(next).forEach(function(path){setPath(res.config,path,next[path]);});
      return P.put('/api/config',{config:res.config,revision:res.revision});
    }).then(function(res){
      state.config=res.config;state.revision=res.revision;paintScope(scope,res.config);setRev();banner('');
      $('status-'+scope).textContent='本分区已保存；其他分区草稿未提交';P.toast(labels[scope]+'已保存','ok');
      if(window.PAIGU_SHELL)window.PAIGU_SHELL.refresh();
      return res;
    }).catch(function(e){$('status-'+scope).textContent='未保存，输入保留：'+P.errorText(e);banner(labels[scope]+'保存未完成：'+P.errorText(e),true);throw e;})
      .finally(function(){nodes.forEach(function(n,i){n.disabled=disabled[i];});delete state.saving[scope];});
    return state.saving[scope];
  }
  function load(){
    if(state.loadPromise)return state.loadPromise;
    setConn('','读取中…');
    $('form').inert=true;
    var controls=Array.from($('form').querySelectorAll('input,select,textarea,button')), disabled=controls.map(function(n){var value=n.disabled;n.disabled=true;return value;});
    state.loadPromise=P.get('/api/config').then(function(res){
      state.config=res.config;state.revision=res.revision;Object.keys(fields).forEach(function(scope){paintScope(scope,res.config);});
      state.importPreflight=null;$('snap-apply').disabled=true;setRev();banner('');setConn('ok','已连接');
      $('form').classList.remove('hidden');$('loading').classList.add('hidden');
    }).catch(function(e){banner('读取失败，保留现有输入：'+P.errorText(e),true);setConn('bad','未连接');$('loading').textContent='读取失败，可重试';throw e;})
      .finally(function(){$('form').inert=false;controls.forEach(function(n,i){n.disabled=disabled[i];});$('snap-apply').disabled=!state.importPreflight;delete state.loadPromise;});
    return state.loadPromise;
  }
  function reloadDisk(){
    return P.protectAction(function(){
      $('reload').disabled=true;
      return P.post('/api/config/reload',{}).then(load).then(function(){P.toast('已从磁盘重载服务配置','ok');})
        .catch(function(e){banner('磁盘重载未完成：'+P.errorText(e),true);throw e;}).finally(function(){$('reload').disabled=false;});
    }).catch(function(){});
  }
  function exportSnapshot() {
    var out = val('snap-out').trim();
    var body = { format: 'file' };
    if (out) body.out_dir = out;
    $('snap-export').disabled = true;
    $('snap-export-info').textContent = '导出中…';
    P.post('/api/snapshot/export', body).then(function (res) {
      var path = res && res.path ? res.path : '(未返回路径)';
      var extra = '';
      if (res && res.manifest) {
        extra = '（manifest: round_id=' + (res.manifest.round_id || '?') + '，revision=' + (res.manifest.revision != null ? res.manifest.revision : '?') + '）';
      }
      $('snap-export-info').textContent = '已导出：' + path + ' ' + extra;
      if (res && res.path) setVal('snap-path', res.path);
      banner('');
      P.toast('快照已导出', 'ok');
    }).catch(function (err) {
      $('snap-export-info').textContent = '导出失败：' + P.errorText(err);
      banner('快照导出失败：' + P.errorText(err), true);
    }).then(function () { $('snap-export').disabled = false; });
  }

  function importSnapshot(apply, protectedDrafts) {
    if(apply && !protectedDrafts) return P.protectAction(function(){return importSnapshot(true,true);}).catch(function(e){banner(P.errorText(e),true);});
    var path = val('snap-path').trim();
    if (!path) { banner('请填写要导入的 path', true); return; }
    var preflight=state.importPreflight;
    if (apply && (!preflight || preflight.path!==path)) { banner('请先预检当前路径的备份。',true);return; }
    var body = { path: path, apply: !!apply,revision:apply?preflight.config_revision:state.revision };
    if(apply){body.round_id=preflight.target_round_id;body.preflight_token=preflight.preflight_token;}
    $('snap-import').disabled = true;
    $('snap-apply').disabled=true;
    $('snap-import-result').textContent = apply?'正在应用…':'正在预检，不改数据…';
    P.post('/api/snapshot/import', body).then(function (res) {
      var applied = !!(res && res.applied);
      if(!applied && val('snap-path').trim()!==path) { state.importPreflight=null;$('snap-import-result').textContent='路径已变化，请重新预检。';return; }
      state.importPreflight=applied?null:Object.assign({path:path},res);
      $('snap-apply').disabled=applied||!res.applicable;
      $('snap-import-result').textContent = applied?'已应用并重算；原完整日志备份：'+res.backup_path:
        '备份轮次：'+res.manifest.round_id+' → 当前轮：'+res.target_round_id+'；消息 '+res.current_messages+' → '+res.import_messages+'。'+res.effect+(res.applicable?'':' 请先手动激活匹配轮次，再预检。');
      banner('');
      P.toast(applied?'备份已应用':'预检完成，尚未应用', 'ok');
      if (applied && window.PAIGU_SHELL) window.PAIGU_SHELL.refresh();
    }).catch(function (err) {
      if (err.status === 409) banner('导入冲突（HTTP 409）：revision 已变化，请先「重新载入」配置。', true);
      else banner('快照导入失败：' + P.errorText(err), true);
      $('snap-import-result').textContent = '导入失败：' + P.errorText(err);
    }).then(function () { $('snap-import').disabled = false; });
  }


  function init(){
    $('api').value=P.resolveApiBase();
    Object.keys(fields).forEach(function(scope){
      P.registerDraft({id:'settings-'+scope,label:labels[scope],isDirty:function(){return dirty(scope);},save:function(){return saveScope(scope);},discard:function(){restoreScope(scope);}});
      $('save-'+scope).addEventListener('click',function(){saveScope(scope).catch(function(){});});
      document.querySelector('[data-settings-scope="'+scope+'"]').addEventListener('input',function(){$('status-'+scope).textContent=dirty(scope)?'本分区有未保存改动':'与读取值相同';});
    });
    $('load').textContent='重新读取服务器值';
    $('load').addEventListener('click',function(){P.protectAction(load).catch(function(){});});
    $('reload').addEventListener('click',reloadDisk);
    $('snap-export').addEventListener('click',exportSnapshot);
    $('snap-import').addEventListener('click',function(){importSnapshot(false);});
    $('snap-apply').addEventListener('click',function(){importSnapshot(true);});
    $('snap-path').addEventListener('input',function(){state.importPreflight=null;$('snap-apply').disabled=true;});
    load().catch(function(){});
  }
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init);else init();
})();
