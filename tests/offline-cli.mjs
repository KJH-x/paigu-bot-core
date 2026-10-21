import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {isolatedEnvironment,managedDirectory} from '../scripts/check-lib.mjs';
const repository=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'paigu-cli-check-'));
try{
  fs.mkdirSync(path.join(directory,'temp'),{recursive:true});
  const exe=path.join(repository,'target','debug',process.platform==='win32'?'paigu-bot-core.exe':'paigu-bot-core');
  if(!fs.existsSync(exe))throw new Error('Build the debug executable before the public offline CLI check');
  const config={round_id:'public-offline-check',title:'公开离线占位',group_id:'123456789',items:[{item_id:'badge',name:'徽章',kind:'split',subitems:[{subitem_id:'a',name:'甲',capacity:2,unit_price_cents:1000}]}]};
  fs.writeFileSync(path.join(directory,'round.json'),JSON.stringify(config));
  const messages=['排 徽章 甲 2','排 徽章 甲 1','撤 徽章 1'].map((text,i)=>({source_sequence:i+1,group_id:config.group_id,user_id:i===1?'成员02':'成员01',nickname:i===1?'成员02':'成员01',message_id:'public-'+i,timestamp_ms:1000+i*1000,text,attachments:[],reply_to_message_id:null,is_admin:false}));
  fs.writeFileSync(path.join(directory,'queue.jsonl'),messages.map(m=>JSON.stringify(m)).join('\n'));
  const result=spawnSync(exe,['simulate','--round-config',path.join(directory,'round.json'),'--queue',path.join(directory,'queue.jsonl'),'--out',path.join(directory,'out')],{cwd:directory,env:isolatedEnvironment(directory,repository,process.env),encoding:'utf8',timeout:30000,windowsHide:true});
  if(result.status!==0)throw new Error('Public offline CLI failed (no real receiver/model is used)');
  const report=JSON.parse(fs.readFileSync(path.join(directory,'out','result.json'),'utf8'));
  const total=report.final_snapshot.user_summaries.flatMap(u=>u.items).reduce((sum,item)=>sum+item.quantity,0);
  if(report.message_count!==3||total!==2||report.outcomes.some(m=>m.status!=='Applied'))throw new Error('Public offline CLI allocation did not match the three-message claim/cancel contract');
  console.log('PASS public offline CLI: 3 messages, 2 allocated, no NapCat/model/private data');
}finally{fs.rmSync(managedDirectory(directory,os.tmpdir(),'paigu-cli-check-'),{recursive:true,force:true});}
