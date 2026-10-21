import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {isolatedEnvironment,managedDirectory} from '../scripts/check-lib.mjs';
const repository=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'paigu-cli-check-'));
try{
  fs.mkdirSync(path.join(directory,'temp'),{recursive:true});
  const exe=path.join(repository,'target','debug',process.platform==='win32'?'paigu-bot-core.exe':'paigu-bot-core');
  if(!fs.existsSync(exe))throw new Error('Build the debug executable before the public offline CLI check');
  const config={round_id:'public-offline-check',title:'公开离线占位',group_id:'123456789',items:[{item_id:'badge',name:'徽章',kind:'split',subitems:[{subitem_id:'a',name:'甲',capacity:2,unit_price_cents:1000}]}]};
  config.fixture_version=2;
  const unusedCache=path.join(directory,'unused-members.json');fs.writeFileSync(unusedCache,'not JSON; must never be read by offline simulation');
  config.members={cache_path:unusedCache,cn_overrides:[{user_id:'成员01',cn:'成员03',aliases:[]}]};
  fs.writeFileSync(path.join(directory,'round.json'),JSON.stringify(config));
  const messages=['排 徽章 甲 2','排 徽章 甲 1','撤 徽章 1'].map((text,i)=>({source_sequence:i+1,group_id:config.group_id,user_id:i===1?'成员02':'成员01',nickname:i===1?'成员02':'成员01',message_id:'public-'+i,timestamp_ms:1000+i*1000,text,attachments:[],reply_to_message_id:null,is_admin:false}));
  fs.writeFileSync(path.join(directory,'queue.jsonl'),messages.map(m=>JSON.stringify(m)).join('\n'));
  const result=spawnSync(exe,['simulate','--round-config',path.join(directory,'round.json'),'--queue',path.join(directory,'queue.jsonl'),'--out',path.join(directory,'out')],{cwd:directory,env:isolatedEnvironment(directory,repository,process.env),encoding:'utf8',timeout:30000,windowsHide:true});
  if(result.status!==0)throw new Error('Public offline CLI failed (no real receiver/model is used)');
  const report=JSON.parse(fs.readFileSync(path.join(directory,'out','result.json'),'utf8'));
  const total=report.final_snapshot.user_summaries.flatMap(u=>u.items).reduce((sum,item)=>sum+item.quantity,0);
  if(report.message_count!==3||total!==2||report.outcomes.some(m=>m.status!=='Applied'))throw new Error('Public offline CLI allocation did not match the three-message claim/cancel contract');
  if(report.outcomes[0].nickname!=='成员03'||report.final_snapshot.user_summaries.find(u=>u.user_id==='成员01')?.display_name!=='成员03')throw new Error('Explicit fixture CN did not reach both messages and allocation');
  if(report.replay.input_message_count!==3||report.final_snapshot.version!==3||report.replay.steps.at(-1)?.after_version!==3||!report.replay.final_settlement)throw new Error('Current replay count/version/final settlement contract was not honored');
  if(['applied','rejected','need_confirm','ignored','unsupported','duplicate','dropped','merged','error'].reduce((sum,state)=>sum+report[state+'_count'],0)!==report.message_count)throw new Error('Outcome counts did not account for every input message');
  console.log('PASS public offline CLI: 3 messages, 2 allocated, explicit CN, replay metadata, no NapCat/model/private data');
  // Captured from the signed pre-migration 0be05ad executable, not from this renderer.
  const baselines=[
    ['agent-a-normal',52,'c6bb80fc36259f919ebc1bb4afe6beef1ce23bb89e3536d2d7b02d1ba42ca916'],
    ['agent-b-box-tail',49,'19d883e9b1a1d26adfa9606d24a8245666c7c365b7900a3a5d2a2f2f63425c43'],
    ['agent-c-cancel-fund',57,'010af0a32805db3c2477e5722c54837cd7f052a2458151a9f9268a48876c3bab'],
    ['agent-d-adversarial',68,'14d5f2488e1eb0d433da7d21dde5c7dd8a5bc8cb372e7fd25ad2583e92efc11d']
  ];
  const cells=board=>board.item_allocations.map(a=>({id:a.item_id,sub:a.subitem_id,kind:a.kind,boxes:a.boxes.map(c=>({index:c.box_index,slots:c.slots.map(s=>({index:s.slot_index,user:s.user_id,status:s.status,policy:s.slot_policy}))})),singles:a.singles.map(s=>({user:s.user_id,qty:s.quantity})),waiting:a.waiting.map(s=>({user:s.user_id,qty:s.quantity}))}));
  for(const [name,count,expected]of baselines){
    const source=path.join(repository,'simulation-corpus',name),out=path.join(directory,name);
    const run=spawnSync(exe,['simulate','--round-config',path.join(source,'round_config.json'),'--queue',path.join(source,'queue.jsonl'),'--out',out],{cwd:directory,env:isolatedEnvironment(directory,repository,process.env),encoding:'utf8',windowsHide:true,timeout:30000});
    if(run.status!==0)throw new Error('Public corpus execution failed: '+name);
    const actual=JSON.parse(fs.readFileSync(path.join(out,'result.json'),'utf8'));
    const hash=createHash('sha256').update(JSON.stringify(cells(actual.final_snapshot))).digest('hex');
    if(actual.message_count!==count||hash!==expected)throw new Error('Pre-migration public cell baseline changed: '+name);
  }
  console.log('PASS public legacy corpus: 4 sets, 226 messages, full cell/empty/capacity/policy/single/waiting baselines');
}finally{fs.rmSync(managedDirectory(directory,os.tmpdir(),'paigu-cli-check-'),{recursive:true,force:true});}
