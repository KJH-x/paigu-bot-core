(function(){
  'use strict';
  var P=window.PAIGU, M=window.PAIGU_MANAGE, state={config:null,revision:null,members:[],generation:0,suggestions:[]};
  function $(id){return document.getElementById(id);}
  function banner(message,bad){var b=$('banner');b.textContent=message || '';b.className='banner'+(message?' show':'')+(bad?' bad':'');}
  function setRev(){$('rev-badge').textContent='配置 v'+(state.revision==null?'—':state.revision);}
  function val(id){return $(id).value;}
  function sourceFields(config){var m=config.members;return {group_id:m.group_id,cache_path:m.cache_path,daily_pull_at:m.daily_pull_at};}
  function setSource(){var m=state.config.members;$('mb-group').value=m.group_id;$('mb-cache').value=m.cache_path;$('mb-pull').value=m.daily_pull_at;}
  function namesDirty(){return !!state.config && JSON.stringify(collectOverrides())!==state.namesBaseline;}
  function sourceDirty(){return !!state.config && JSON.stringify(sourceDraft())!==state.sourceBaseline;}
  function sourceDraft(){return {group_id:val('mb-group').trim(),cache_path:val('mb-cache').trim(),daily_pull_at:val('mb-pull').trim()};}
  function controlsDisabled(nodes){var values=nodes.map(function(n){var original=n.disabled;n.disabled=true;return original;});return function(){nodes.forEach(function(n,i){n.disabled=values[i];});};}
  function scopedSave(kind){
    if(!state.config)return Promise.reject(new Error('配置尚未载入'));
    var generation=state.generation, scope=kind==='names'?'cn_overrides':'source';
    var previous=kind==='names'?state.config.members.cn_overrides:sourceFields(state.config);
    var next=kind==='names'?collectOverrides():sourceDraft();
    var nodes=Array.from(document.querySelectorAll(kind==='names'?'#members-body input,#members-body button,#members-cn-save':'#member-source input,#members-source-save'));
    var restore=controlsDisabled(nodes);
    return P.get('/api/config').then(function(res){
      if(generation!==state.generation || res.config.round.round_id!==state.config.round.round_id)throw new Error('轮次已切换，当前草稿保留');
      var latest=scope==='source'?sourceFields(res.config):res.config.members.cn_overrides;
      if(JSON.stringify(latest)!==JSON.stringify(previous))throw new Error('同一分区已被其他操作修改，草稿保留；请核对服务器值');
      if(scope==='source')Object.assign(res.config.members,next);else res.config.members.cn_overrides=next;
      return P.put('/api/config',{config:res.config,revision:res.revision});
    }).then(function(res){
      if(generation!==state.generation)return;
      if(scope==='source')Object.assign(state.config.members,next);else state.config.members.cn_overrides=P.deepClone(res.config.members.cn_overrides);
      if(kind==='names')state.namesBaseline=JSON.stringify(collectOverrides());else state.sourceBaseline=JSON.stringify(sourceDraft());
      state.revision=res.revision;setRev();banner('仅'+(kind==='names'?'称呼与别名':'成员来源设置')+'已保存；其他草稿未提交');
      return res;
    }).catch(function(e){banner('保存未完成，草稿保留：'+P.errorText(e),true);throw e;}).finally(restore);
  }
  function collectOverrides() {
    var ids=new Set((state.members || []).map(function(m){return m.user_id;}));
    var out = ((state.config && state.config.members && state.config.members.cn_overrides) || []).filter(function(o){return !ids.has(o.user_id);});
    (state.members || []).forEach(function (m) {
      if (!m.user_id) return;
      var aliases = M.dedupe(m.aliases || []);
      // 未编辑的 CN 由前端自动填充（=清洗后群名片），不落库；仅持久化用户编辑过的 CN / 别名。
      if (!m.cnTouched && !aliases.length) return;
      out.push({ user_id: m.user_id, cn: m.cnTouched ? (m.cn || '').trim() : '', aliases: aliases });
    });
    return out;
  }

  function buildMembers(res) {
    var raw = (res && res.members) || [];
    var overrides = (state.config && state.config.members && state.config.members.cn_overrides) || [];
    var byId = {};
    overrides.forEach(function (o) { byId[o.user_id] = o; });
    return raw.map(function (m) {
      m = m || {};
      var uid = m.user_id != null ? String(m.user_id) : '';
      var ov = byId[uid];
      var card = m.card != null ? String(m.card) : '';
      var rawName = card.trim() ? card : (m.nickname != null ? String(m.nickname) : '');
      var cleaned = P.cleanNickname(rawName);
      return {
        user_id: uid,
        nickname: m.nickname != null ? String(m.nickname) : '',
        raw: rawName,
        cleaned: cleaned,
        role: m.role || '',
        resolved: m.resolved != null ? String(m.resolved) : '',
        cn: ov && ov.cn ? ov.cn : cleaned,
        cnTouched: !!(ov && ov.cn),
        aliases: ov ? M.asStrArray(ov.aliases) : [],
        fallback: !!m.fallback
      };
    });
  }

  function pinyinText(text) {
    var out = [];
    if (window.pinyinPro) {
      try { out = window.pinyinPro.pinyin(String(text || ''), { toneType: 'none', type: 'array' }) || []; }
      catch (e) { out = []; }
    }
    return out.join('');
  }

  function pinyinInitials(text) {
    if (!window.pinyinPro) return '';
    try {
      return window.pinyinPro.pinyin(String(text || ''), { pattern: 'first', toneType: 'none', type: 'array' }).join('');
    } catch (e) { return ''; }
  }

  function memberHaystack(m) {
    var base = [m.user_id, m.raw, m.cleaned, m.cn, m.resolved, (m.aliases || []).join(' ')].join(' ');
    return (base + ' ' + pinyinText(base) + ' ' + pinyinInitials(base)).toLowerCase();
  }

  function renderMembers() {
    var tbody = $('members-body');
    if (!tbody) return;
    while (tbody.firstChild) tbody.removeChild(tbody.firstChild);
    var filterEl = $('members-filter');
    var q = filterEl ? filterEl.value.trim().toLowerCase() : '';
    var list = state.members || [];
    var shown = 0;
    list.forEach(function (m, i) {
      if (q && memberHaystack(m).indexOf(q) < 0) return;
      shown++;
      var tr = document.createElement('tr');
      tr.innerHTML =
        '<td><details><summary aria-label="查看成员标识">标识</summary><code>' + P.esc(m.user_id || '未提供') + '</code></details></td>' +
        '<td>' + P.esc(m.raw || '') + '</td>' +
        '<td class="member-clean-preview">' + P.esc(m.cleaned || '') + '</td>' +
        '<td><input aria-label="最终称呼" class="cn" ' + (!m.user_id ? 'disabled title="占位示例没有成员标识，不能保存" ' : '') + 'data-user="' + P.esc(m.user_id) + '" value="' + P.esc(m.cn || '') + '" placeholder="' + P.esc(m.resolved || m.cleaned || '') + '" /></td>' +
        '<td><input aria-label="成员别名" class="aliases" ' + (!m.user_id ? 'disabled ' : '') + 'data-user="' + P.esc(m.user_id) + '" value="' + P.esc((m.aliases || []).join(', ')) + '" placeholder="别名，逗号分隔" /></td>' +
        '<td>' + P.esc(m.role || '') + '</td>' +
        '<td><button type="button" class="small" ' + (!m.user_id ? 'disabled ' : '') + 'data-use="' + i + '">恢复自动</button><span class="hint">' + (!m.user_id ? '只读占位示例' : (m.cnTouched ? '自定义' : '自动跟随群名片')) + '</span></td>';
      tbody.appendChild(tr);
    });
    $('members-empty').hidden = shown > 0;
    $('members-info').textContent = (q ? shown + ' / ' : '') + list.length + ' 人' + (list.length && list[0].fallback ? '（内置子集，未接入群）' : '');
  }

  function onMembersInput(ev) {
    var t = ev.target;
    if (!t || !t.dataset || !t.dataset.user) return;
    var uid = t.dataset.user;
    var entry = null;
    for (var i = 0; i < state.members.length; i++) {
      if (state.members[i].user_id === uid) { entry = state.members[i]; break; }
    }
    if (!entry) return;
    if (t.classList.contains('aliases')) entry.aliases = M.splitTokens(t.value);
    else { entry.cn = t.value; entry.cnTouched = true; }
    var status=t.closest('tr').querySelector('.hint');if(status)status.textContent=entry.cnTouched?'自定义':'自动跟随群名片';
  }

  function onMembersClick(ev) {
    var b = ev.target.closest ? ev.target.closest('button[data-use]') : null;
    if (!b) return;
    var i = parseInt(b.getAttribute('data-use'), 10);
    var m = state.members[i];
    if (!m) return;
    m.cn = m.cleaned || m.resolved || m.nickname || '';
    m.cnTouched = false;
    renderMembers();
    var restored=$('members-body').querySelector('button[data-use="'+i+'"]');if(restored)restored.focus();
  }


  function load(){
    var generation=++state.generation;
    return Promise.all([P.get('/api/config'),P.get('/api/members')]).then(function(res){
      if(generation!==state.generation)return;
      var config=res[0], members=res[1];
      if(members.round_id && (members.round_id!==config.config.round.round_id || members.config_revision!==config.revision))throw new Error('读取期间配置已变化，请重新读取');
      state.config=config.config;state.revision=config.revision;state.members=buildMembers(members);
      renderMembers();setSource();setRev();state.namesBaseline=JSON.stringify(collectOverrides());state.sourceBaseline=JSON.stringify(sourceDraft());
      $('form').classList.remove('hidden');$('loading').classList.add('hidden');
      $('members-cn-save').disabled=!state.members.some(function(m){return !!m.user_id;});
      var sources={cache:'本机缓存',seed:'本机种子',example:'只读占位示例',empty:'暂无名单'};
      $('members-source-info').textContent='来源：'+(sources[members.source] || members.source)+' · 缓存所属群：'+(members.cached_group_id || '未记录，无法验证')+
        ' · 配置群：'+state.config.members.group_id+' · 激活轮群：'+state.config.round.group_id+
        (members.cache_updated_at_ms?' · 文件更新：'+new Date(members.cache_updated_at_ms).toLocaleString():'')+
        (members.cached_group_id && members.cached_group_id!==state.config.round.group_id?' · 注意：缓存与激活轮群不同，不会自动拉取或覆盖':'');
      $('members-source-info').classList.toggle('warning',!!members.cached_group_id && members.cached_group_id!==state.config.round.group_id);
      $('members-cn-llm').disabled=!state.config.llm.enabled;
      $('members-cn-llm').title=state.config.llm.enabled?'仅主动点击才请求模型，先预览，不覆盖草稿':'模型未启用，仍可手工编辑称呼';
      $('conn-badge').textContent='已读取成员';$('conn-badge').className='status-badge ok';banner('');
    }).catch(function(e){banner('成员载入未完成：'+P.errorText(e),true);$('loading').textContent='载入失败，可重新读取';throw e;});
  }
  function refreshMembers(){
    return P.protectAction(refreshMembersNow).catch(function(){});
  }
  function refreshMembersNow(){
    var btn=$('members-refresh');btn.disabled=true;
    return P.post('/api/members/refresh',{}).then(load).catch(function(e){banner('拉取失败，保留现有名单与草稿：'+P.errorText(e)+'。需要已连接的接收器。',true);}).finally(function(){btn.disabled=false;});
  }
  function suggest(){
    var btn=$('members-cn-llm'), generation=state.generation;
    var baseline=state.members.map(function(m){return {user_id:m.user_id,cn:m.cn,cnTouched:m.cnTouched,raw:m.raw};});
    btn.disabled=true;
    $('members-ai-info').textContent='请求中；只发送有标识的群名片和成员标识，不发送聊天或 NC 原事件。服务：'+state.config.llm.base_url;
    $('member-suggestions').hidden=false;
    P.post('/api/members/cn-suggest',{}).then(function(res){
      if(generation!==state.generation)return;
      state.suggestions=(res.suggestions || []).map(function(s){return Object.assign({},s,{before:baseline.find(function(m){return m.user_id===s.user_id;})});}).filter(function(s){return s.before;});
      $('members-ai-body').innerHTML=state.suggestions.map(function(s,i){return '<tr><td><input type="checkbox" aria-label="采用 '+P.esc(s.before.raw)+' 的建议" data-suggestion="'+i+'" '+(!s.before.cnTouched?'checked':'')+' /></td><td>'+P.esc(s.before.raw)+'</td><td>'+P.esc(s.before.cn)+'</td><td>'+P.esc(s.cn)+'</td></tr>';}).join('');
      $('members-ai-info').textContent='建议 '+state.suggestions.length+' 条，尚未修改草稿。人工称呼默认不勾选；请求后编辑过的称呼也不会被覆盖。';
    }).catch(function(e){$('members-ai-info').textContent='建议未完成，草稿未改：'+P.errorText(e);}).finally(function(){if(generation===state.generation)btn.disabled=!state.config.llm.enabled;});
  }
  function applySuggestions(){
    var changed=0, skipped=0;
    $('members-ai-body').querySelectorAll('input:checked').forEach(function(n){
      var s=state.suggestions[Number(n.dataset.suggestion)], member=state.members.find(function(m){return m.user_id===s.user_id;});
      if(!member || member.cn!==s.before.cn || member.cnTouched!==s.before.cnTouched){skipped++;return;}
      member.cn=s.cn;member.cnTouched=true;changed++;
    });
    renderMembers();$('member-suggestions').hidden=true;banner('已填入草稿 '+changed+' 条，跳过期间已改 '+skipped+' 条；保存后才生效');
  }
  function init(){
    $('api').value=P.resolveApiBase();
    P.registerDraft({id:'members-names',label:'成员称呼与别名',isDirty:namesDirty,save:function(){return scopedSave('names');},discard:function(){
      state.members=buildMembers({members:state.members.map(function(m){return {user_id:m.user_id,card:m.raw,nickname:m.nickname,role:m.role,resolved:m.resolved,fallback:m.fallback};})});renderMembers();state.namesBaseline=JSON.stringify(collectOverrides());
    }});
    P.registerDraft({id:'members-source',label:'成员数据来源',isDirty:sourceDirty,save:function(){return scopedSave('source');},discard:function(){setSource();}});
    $('load').addEventListener('click',function(){P.protectAction(load).catch(function(){});});
    $('members-cn-save').addEventListener('click',function(){scopedSave('names').catch(function(){});});
    $('members-source-save').addEventListener('click',function(){scopedSave('source').catch(function(){});});
    $('members-body').addEventListener('input',onMembersInput);$('members-body').addEventListener('click',onMembersClick);
    $('members-filter').setAttribute('aria-label','搜索成员群名片、称呼或别名');$('members-filter').addEventListener('input',renderMembers);
    $('members-refresh').addEventListener('click',refreshMembers);$('members-cn-llm').addEventListener('click',suggest);
    $('members-ai-apply').addEventListener('click',applySuggestions);$('members-ai-dismiss').addEventListener('click',function(){$('member-suggestions').hidden=true;});
    load().catch(function(){});
  }
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',init);else init();
})();
