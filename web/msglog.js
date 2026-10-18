(function(global){
  'use strict';
  var P=global.PAIGU;
  function create(opts){
    function $(id){return document.getElementById(id);}
    var state={messages:[],roundId:'',configRevision:0,generation:0,since:0,total:0};
    var more=document.createElement('button');more.type='button';more.className='small';more.textContent='加载后续记录';more.hidden=true;
    var host=$(opts.tbody);if(host)host.closest('.table-wrap').after(more);
    function readonly(){return document.getElementById('source') && document.getElementById('source').value!=='local';}
    function cell(row,value,cls){var td=document.createElement('td');td.textContent=value==null?'—':value;if(cls)td.className=cls;row.appendChild(td);return td;}
    function render(){
      var tbody=$(opts.tbody);if(!tbody)return;tbody.replaceChildren();
      state.messages.forEach(function(m){
        var tr=document.createElement('tr');tr.dataset.recordId=m.record_id;
        var identity=cell(tr,'#'+m.seq,'mono');
        var detail=document.createElement('details'), summary=document.createElement('summary');summary.textContent='记录信息';detail.appendChild(summary);
        var id=document.createElement('code');id.textContent=m.record_id+' · v'+m.record_revision;detail.appendChild(id);identity.appendChild(detail);
        cell(tr,m.text+'\n'+(m.nickname || m.user_id),'message-raw');
        var time=cell(tr,new Date(m.timestamp_ms).toLocaleString());time.title='原消息时间，只读';
        cell(tr,(m.status || '')+'\n'+(m.detail || m.routed || ''),'message-raw');
        var action=cell(tr,''), button=document.createElement('button');button.type='button';button.className='small';
        button.textContent=readonly()?'远程来源 · 只读':'查看 / 修订动作';button.disabled=readonly() || !m.record_id || !m.round_id || !m.record_revision;
        button.setAttribute('aria-label','查看或修订 '+(m.nickname || m.user_id)+' 的消息 #'+m.seq);
        button.addEventListener('click',function(){
          if(readonly())return;
          P.openMessageEditor({apiBase:P.resolveApiBase(),message:Object.assign({display:m.nickname || m.user_id},m),onSaved:function(){load(false);var refresh=document.getElementById('refresh');if(refresh)refresh.click();}});
        });action.appendChild(button);tbody.appendChild(tr);
      });
      if($(opts.empty))$(opts.empty).hidden=state.messages.length>0;
      if($(opts.info))$(opts.info).textContent=state.messages.length+' 条已载入'+(readonly()?' · 远程查看时不提供本机修订':'');
      more.hidden=readonly() || !state.more;
    }
    function load(append){
      var generation=++state.generation, refresh=$(opts.refresh), base=P.resolveApiBase();
      if(readonly()){state.messages=[];state.more=false;render();if($(opts.empty))$(opts.empty).textContent='远程快照只读，不在这里读取本机私有消息日志';return Promise.resolve();}
      if(refresh)refresh.disabled=true;more.disabled=true;
      var since=append?state.since:0;
      return P.get(base+'/api/messages?limit=500&since='+since).then(function(res){
        if(generation!==state.generation || base!==P.resolveApiBase())return;
        if(append && state.roundId!==res.round_id)throw new Error('轮次已切换，请刷新消息记录');
        state.roundId=res.round_id;state.configRevision=res.config_revision;
        var rows=res.messages || [];state.messages=append?state.messages.concat(rows):rows;
        state.since=rows.length?rows[rows.length-1].seq:since;state.more=res.total>rows.length;
        render();
      }).catch(function(err){if(opts.onBanner)opts.onBanner('消息记录载入未完成：'+P.errorText(err),true);})
        .finally(function(){if(generation===state.generation){if(refresh)refresh.disabled=false;more.disabled=false;}});
    }
    more.addEventListener('click',function(){load(true);});
    if($(opts.refresh))$(opts.refresh).addEventListener('click',function(){load(false);});
    var source=document.getElementById('source');if(source)source.addEventListener('change',function(){load(false);});
    return {load:load,render:render,state:state};
  }
  global.PAIGU_MSGLOG={create:create};
})(window);
