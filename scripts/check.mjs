import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {options,isolatedEnvironment,testExecutables,managedDirectory,runSteps,cargoDiagnostics,REFERENCE_TEST,REFERENCE_PATH} from './check-lib.mjs';

const repository=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const node=process.execPath,manifest=path.join(repository,'Cargo.toml');
let active=null,interrupted=false;
function stop(){
  interrupted=true;
  if(!active?.pid)return;
  if(process.platform==='win32'){
    const stopper=spawn('taskkill',['/PID',String(active.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});stopper.on('error',()=>active?.kill());
  }else{try{process.kill(-active.pid,'SIGTERM');}catch{active.kill();}}
}
process.on('SIGINT',stop);process.on('SIGTERM',stop);
async function execute(label,command,args,{cwd,env,capture=false,privateOutput=false}={}){
  if(interrupted)throw new Error('Check interrupted');
  console.log('CHECK '+label);
  return new Promise((resolve,reject)=>{
    const child=spawn(command,args,{cwd,env,windowsHide:true,detached:process.platform!=='win32',stdio:['ignore',capture?'pipe':'inherit','pipe']});active=child;
    const stdout=[],stderr=[];let bytes=0;
    const collect=(parts,chunk)=>{bytes+=chunk.length;if(bytes>32*1024*1024){child.kill();return;}parts.push(chunk);};
    child.stdout?.on('data',chunk=>collect(stdout,chunk));
    child.stderr.on('data',chunk=>{collect(stderr,chunk);if(!privateOutput)process.stderr.write(chunk);});
    child.on('error',error=>{if(active===child)active=null;reject(new Error(label+' could not start ('+error.code+')'));});
    child.on('close',code=>{
      if(active===child)active=null;
      if(code!==0||interrupted||bytes>32*1024*1024){
        if(capture&&!privateOutput&&command==='cargo')process.stderr.write(cargoDiagnostics(Buffer.concat(stdout).toString('utf8')));
        reject(new Error(label+' failed; later checks were not run'+(privateOutput?' (private diagnostics withheld)':'')));return;
      }
      resolve({stdout:Buffer.concat(stdout).toString('utf8'),stderr:Buffer.concat(stderr).toString('utf8')});
    });
  });
}

async function main(){
  const selected=options(process.argv.slice(2));
  if(selected.help){console.log('Usage: node scripts/check.mjs [--ui] [--samples <private-root>] [--xlsx] [--settlement-fixture <private-json>]\nDefault: isolated core, script, public offline CLI and privacy checks. Private inputs are opt-in; omitted inputs are NOT RUN, never a pass.');return;}
  // Fail on explicitly requested missing inputs before any expensive build.
  if(selected.samples)for(const sample of ['sample1','sample2'])for(const file of ['round_config.json','queue.jsonl','expected_allocation.json'])if(!fs.existsSync(path.join(selected.samples,sample,file)))throw new Error('Requested private samples are incomplete (input path withheld)');
  if(selected.settlementFixture&&!fs.existsSync(selected.settlementFixture))throw new Error('Requested private settlement fixture does not exist (input path withheld)');
  const reference=selected.settlementFixture||path.join(repository,REFERENCE_PATH);
  if(!fs.existsSync(reference))throw new Error('Bundled settlement reference is missing; restore the tracked fixture or supply --settlement-fixture');
  const parent=os.tmpdir(),directory=fs.mkdtempSync(path.join(parent,'paigu-check-'));
  const env=isolatedEnvironment(directory,repository,process.env),cwd=directory;
  try{
    fs.mkdirSync(path.join(directory,'temp'),{recursive:true});
    const config=JSON.parse(fs.readFileSync(path.join(repository,'config.example.json'),'utf8'));
    delete env[config.llm.api_key_env];
    config.llm.enabled=false;config.gateway.reply_enabled=false;config.gateway.admin_commands_enabled=false;config.gateway.bind='127.0.0.1:0';
    config.members.cache_path=path.join(directory,'runtime','members.json');config.members.cn_overrides=[];
    fs.mkdirSync(path.dirname(env.PAIGU_CONFIG_PATH),{recursive:true});
    fs.writeFileSync(env.PAIGU_CONFIG_PATH,JSON.stringify(config));
    fs.writeFileSync(path.join(directory,'config.example.json'),JSON.stringify(config));
    const jobs=[
      ['format',()=>execute('Rust format','cargo',['fmt','--manifest-path',manifest,'--check'],{cwd,env})],
      ['lint',()=>execute('Rust clippy','cargo',['clippy','--manifest-path',manifest,'--all-targets','--','-D','warnings'],{cwd,env})],
      ['build',()=>execute('Rust build','cargo',['build','--manifest-path',manifest],{cwd,env})],
      ['core',async()=>{
        const build=await execute('Compile Rust tests','cargo',['test','--manifest-path',manifest,'--no-run','--message-format=json'],{cwd,env,capture:true});
        const binaries=testExecutables(build.stdout,env.CARGO_TARGET_DIR);
        for(const binary of binaries)await execute('Rust core in isolated working directory',binary.executable,['--color','never','--quiet','--skip',REFERENCE_TEST],{cwd,env});
        const destination=path.join(directory,REFERENCE_PATH);fs.mkdirSync(path.dirname(destination),{recursive:true});fs.copyFileSync(reference,destination);
        const main=binaries.find(b=>b.target==='paigu-bot-core');if(!main)throw new Error('Reference test executable was not found');
        const result=await execute(selected.settlementFixture?'Explicit settlement reference':'Bundled settlement reference',main.executable,['--color','never','--exact',REFERENCE_TEST],{cwd,env,capture:true,privateOutput:true});
        if(!/^test result: ok\. 1 passed;/m.test(result.stdout))throw new Error('Settlement fixture did not actually run its single reference test');
        console.log('PASS settlement reference (1 test; in addition to the core run above)');
      }],
      ['scripts',()=>execute('Check runner self-tests',node,['--test',path.join(repository,'tests','check-runner.mjs')],{cwd,env})],
      ['docs',()=>execute('Current document links and source references',node,['--test',path.join(repository,'tests','docs.mjs')],{cwd,env})],
      ['offline',()=>execute('Public offline CLI',node,[path.join(repository,'tests','offline-cli.mjs')],{cwd,env})],
      ['privacy-tests',()=>execute('Privacy guard self-tests',node,[path.join(repository,'tests','privacy-scan.mjs')],{cwd,env})],
      ['privacy',()=>execute('Tracked/index privacy scan',node,[path.join(repository,'scripts','privacy-scan.mjs')],{cwd:repository,env})]
    ];
    await runSteps(jobs,async ([,run])=>run());
    if(selected.ui)await execute('Complete desktop suite (no real receiver/model)',node,[path.join(repository,'tests','e2e','sim.mjs')],{cwd:repository,env});
    else console.log('NOT RUN: desktop UI (use --ui; this is not a UI pass)');
    if(selected.samples){
      const samples=path.join(directory,'private-samples');
      for(const sample of ['sample1','sample2']){
        const destination=path.join(samples,sample);fs.mkdirSync(destination,{recursive:true});
        for(const file of ['round_config.json','queue.jsonl','expected_allocation.json'])fs.copyFileSync(path.join(selected.samples,sample,file),path.join(destination,file));
        const exe=path.join(env.CARGO_TARGET_DIR,'debug',process.platform==='win32'?'paigu-bot-core.exe':'paigu-bot-core');
        await execute('Private sample simulation',exe,['simulate','--round-config',path.join(destination,'round_config.json'),'--queue',path.join(destination,'queue.jsonl'),'--out',path.join(destination,'out')],{cwd,env,capture:true,privateOutput:true});
      }
      await execute('Private sample cell comparison',node,[path.join(repository,'simulation-corpus','real-samples','verify-samples.mjs'),samples],{cwd,env,capture:true,privateOutput:true});
      console.log('PASS explicit private samples (both sample sets)');
    }else console.log('NOT RUN: private sample corpus (use --samples; omission is not a pass)');
    if(selected.xlsx)await execute('Read-only workbook corpus','python',[path.join(repository,'tests','read-only-corpus.py')],{cwd,env});
    else console.log('NOT RUN: private workbook corpus (use --xlsx; omission is not a pass)');
    console.log('PASS all requested check gates; no deployment, live receiver, real model or private runtime mutation');
  }finally{
    fs.rmSync(managedDirectory(directory,parent,'paigu-check-'),{recursive:true,force:true});
  }
}
main().catch(error=>{console.error('CHECK FAILED: '+error.message);process.exitCode=interrupted?130:1;});
