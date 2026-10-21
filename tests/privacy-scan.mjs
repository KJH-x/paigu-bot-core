import {execFileSync,spawnSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const scanner=fs.readFileSync(path.join(root,'scripts/privacy-scan.mjs'));
const names={card:'PrivacySentinelCardLong',cn:'隐私哨兵称呼甲',alias:'隐私哨兵别名乙',nick:'PrivacySentinelNicknameLong'};
const cases=[
  {name:'public checkout without private data',probe:'成员01 public placeholder',status:0},
  {name:'public template values remain safe',probe:'123456789',round:{round_id:'private-test',group_id:'123456789',priority_users:['123456789']},status:0},
  {name:'generic formatting word is not an identifying name',probe:'空格',members:[{nickname:'空格'}],status:0},
  {name:'existing nickname source',probe:names.nick,members:[{nickname:names.nick}],status:1},
  {name:'card-first normalized source',probe:names.card,members:[{card:names.card+' (note)',nickname:'成员01'}],status:1},
  {name:'configured cache wrapper',probe:names.card,cache:'private/cache.json',members:{members:[{card:names.card}]},status:1},
  {name:'explicit CN source',probe:names.cn,overrides:[{user_id:'成员01',cn:names.cn,aliases:[]}],status:1},
  {name:'member alias source',probe:names.alias,overrides:[{user_id:'成员01',cn:'成员01',aliases:[names.alias]}],status:1},
  {name:'split round private ID',probe:'800000099',round:{round_id:'private-test',group_id:'800000099'},status:1},
  {name:'editing example cannot bless a private ID',exampleGroup:'800000099',probe:'800000099',round:{round_id:'private-test',group_id:'800000099'},status:1},
  {name:'numeric cached user ID',probe:'800000098',members:[{user_id:800000098,nickname:'成员01'}],status:1},
  {name:'malformed private source fails closed',probe:'public placeholder',members:'invalid-json',status:1},
  {name:'secret shape',probe:['sk','Q'.repeat(24)].join('-'),status:1},
  {name:'private address shape',probe:['192','168','8','9'].join('.'),status:1},
  {name:'vendored secrets are not exempt',probePath:'web/vendor/fixture.js',probe:['sk','Q'.repeat(24)].join('-'),status:1},
  {name:'scanner itself is not exempt from secrets',scannerProbe:true,probe:['sk','Q'.repeat(24)].join('-'),status:1},
  {name:'vendor dictionary nickname is not treated as a leak',probePath:'web/vendor/dictionary.js',probe:names.nick,members:[{nickname:names.nick}],status:0},
  {name:'staged secret cannot be hidden by clean working copy',probe:['sk','Q'.repeat(24)].join('-'),working:'public placeholder',status:1},
  {name:'staged secret cannot be hidden by missing working file',probe:['sk','Q'.repeat(24)].join('-'),removeWorking:true,status:1},
  {name:'forbidden tracked private path',probePath:'data/fixture.txt',probe:'public placeholder',status:1},
  {name:'unusual tracked filename is scanned exactly',probePath:process.platform==='win32'?'含空格 fixture.txt':'含空格\nfixture.txt',probe:['sk','Q'.repeat(24)].join('-'),status:1},
];
let failures=0;
for(const test of cases){
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'paigu-privacy-test-'));
  const write=(relative,value)=>{const file=path.join(directory,relative);fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,typeof value==='string'?value:JSON.stringify(value));};
  try{
    write('scripts/privacy-scan.mjs',scanner.toString()+(test.scannerProbe?'\n// '+test.probe:''));
    write('.gitignore','config/\ndata/\nprivate/\n');
    write('config.example.json',{members:{cache_path:'data/members.json',cn_overrides:[]},round:{group_id:test.exampleGroup || '123456789'}});
    if(test.members!==undefined)write(test.cache || 'data/members.json',test.members);
    if(test.cache||test.overrides||test.round)write('config/app.json',{active_round_id:test.round?.round_id,members:{cache_path:test.cache || 'data/members.json',cn_overrides:test.overrides || []}});
    if(test.round)write('data/rounds/private-test.json',test.round);
    const probePath=test.probePath || 'probe.txt';
    write(probePath,test.probe);
    execFileSync('git',['init','--quiet'],{cwd:directory,stdio:'pipe'});
    execFileSync('git',['add','-f','--',probePath,'config.example.json'],{cwd:directory,stdio:'pipe'});
    if(test.scannerProbe)execFileSync('git',['add','--','scripts/privacy-scan.mjs'],{cwd:directory,stdio:'pipe'});
    if(test.working!==undefined)write(probePath,test.working);
    if(test.removeWorking)fs.rmSync(path.join(directory,probePath));
    const result=spawnSync(process.execPath,[path.join(directory,'scripts/privacy-scan.mjs')],{cwd:directory,encoding:'utf8'});
    if(result.status!==test.status){failures++;console.error('FAIL '+test.name+': expected exit '+test.status+', got '+result.status);}
    else if(test.status===1&&!result.stderr.includes('  [')){failures++;console.error('FAIL '+test.name+': expected a concrete finding, not an unrelated scanner error');}
    else if(test.status===0&&!result.stdout.includes('privacy-scan: OK')){failures++;console.error('FAIL '+test.name+': scanner did not complete its checks');}
    else if(test.status===1&&(result.stdout+result.stderr).includes(test.probe)){failures++;console.error('FAIL '+test.name+': scanner disclosed matched private content');}
    else console.log('PASS '+test.name);
  }finally{
    const resolved=path.resolve(directory),parent=path.resolve(os.tmpdir());
    if(path.dirname(resolved)!==parent||!path.basename(resolved).startsWith('paigu-privacy-test-'))throw new Error('unsafe fixture cleanup path');
    fs.rmSync(resolved,{recursive:true,force:true});
  }
}
if(failures)throw new Error(failures+' privacy guard cases failed');
console.log('ALL PASS ('+cases.length+' isolated privacy cases)');
