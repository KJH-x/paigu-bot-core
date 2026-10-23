(function () {
  'use strict';
  var P=window.PAIGU, M=window.PAIGU_MANAGE, backupUI=null, state={config:null,revision:null,baselines:{},server:{},saving:{}};
  function $(id){return document.getElementById(id);}
  function banner(message,bad){var b=$('banner');b.textContent=message || '';b.className='banner'+(message?' show':'')+(bad?' bad':'');}
  function val(id){return $(id).value;}
  function setVal(id,value){$(id).value=value==null?'':value;}
  function setRev(){$('rev-badge').textContent='配置 v'+(state.revision==null?'—':state.revision);}
  function setConn(kind,text){$('conn-badge').textContent=text;$('conn-badge').className='status-badge '+kind;}
  function paintIntake(workflow){
    state.intakeContext=workflow?{round_id:workflow.round_id,config_revision:workflow.revision}:null;
    var intake=workflow&&workflow.gateway&&workflow.gateway.intake;
    $('intake-status').textContent=!intake?'消息处理状态不可读':
      (intake.paused||intake.last_error?'处理已暂停：'+(intake.last_error||'等待恢复'):
      intake.worker_running?'处理器运行中':'处理器尚未启动')+'；本轮待处理 '+((intake.pending_by_round||{})[workflow.round_id]||0)+' 条，总待处理 '+intake.pending+' 条';
    $('intake-retry').disabled=!!state.retryingIntake||!intake||!(intake.paused||intake.last_error);
  }
  function retryIntake(){
    if(state.retryingIntake||!state.intakeContext)return;
    state.retryingIntake=true;$('intake-retry').disabled=true;
    P.post('/api/gateway/intake/retry',state.intakeContext).then(function(){
      P.toast('已请求继续未完成消息；请检查处理状态','ok');
    }).catch(function(error){banner('未恢复：'+P.errorText(error),true);})
      .finally(function(){state.retryingIntake=false;if(window.PAIGU_SHELL)window.PAIGU_SHELL.refresh();});
  }
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
      if(backupUI)backupUI.reset();setRev();banner('');setConn('ok','已连接');
      $('form').classList.remove('hidden');$('loading').classList.add('hidden');
    }).catch(function(e){banner('读取失败，保留现有输入：'+P.errorText(e),true);setConn('bad','未连接');$('loading').textContent='读取失败，可重试';throw e;})
      .finally(function(){$('form').inert=false;controls.forEach(function(n,i){n.disabled=disabled[i];});if(backupUI)backupUI.refresh();delete state.loadPromise;});
    return state.loadPromise;
  }
  function reloadDisk(){
    return P.protectAction(function(){
      $('reload').disabled=true;
      return P.post('/api/config/reload',{}).then(load).then(function(){P.toast('已从磁盘重载服务配置','ok');})
        .catch(function(e){banner('磁盘重载未完成：'+P.errorText(e),true);throw e;}).finally(function(){$('reload').disabled=false;});
    }).catch(function(){});
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
    $('intake-retry').addEventListener('click',retryIntake);
    if(window.PAIGU_SHELL)window.PAIGU_SHELL.onWorkflow(paintIntake);
    backupUI=P.mountPrivateBackups({context:function(){return state.config?{round_id:state.config.round.round_id,revision:state.revision}:null;},report:banner,onApplied:load});
    load().catch(function(){});
  }
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init);else init();
})();
