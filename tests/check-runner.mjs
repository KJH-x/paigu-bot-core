import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import os from 'node:os';
import fs from 'node:fs';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {options,isolatedEnvironment,testExecutables,managedDirectory,runSteps,cargoDiagnostics,REFERENCE_PATH} from '../scripts/check-lib.mjs';

test('check flags are explicit and paths are resolved without changing the caller',()=>{
  const cwd=path.resolve('fixture-root');
  assert.deepEqual(options([],cwd),{ui:false,xlsx:false,help:false,samples:null,settlementFixture:null});
  const chosen=options(['--ui','--samples','private','--xlsx','--settlement-fixture','fixture.json'],cwd);
  assert.equal(chosen.samples,path.join(cwd,'private'));assert.equal(chosen.settlementFixture,path.join(cwd,'fixture.json'));
  assert.equal(chosen.ui,true);assert.equal(chosen.xlsx,true);
  assert.throws(()=>options(['--unknown']),/Unknown/);
  assert.throws(()=>options(['--samples','--ui']),/requires/);
});
test('all runtime stores, OS temp and UI filters are isolated, without mutating parent env',()=>{
  const directory=path.resolve('owned-check'),repo=path.resolve('source');
  const original={PATH:'public-path',PAIGU_CONFIG_PATH:'unwanted',PAIGU_ROUNDS_DIR:'unwanted',PAIGU_SNAPSHOT_DIR:'unwanted',PAIGU_UI_CAPTURE_DIR:'unwanted',PAIGU_E2E_FILTER:'subset',DEEPSEEK_API_KEY:'not-real',PAIGU_TEST_KEY_UNSET:'not-real'};
  const env=isolatedEnvironment(directory,repo,original);
  for(const key of ['PAIGU_CONFIG_PATH','PAIGU_ROUNDS_DIR','PAIGU_MESSAGES_DIR','PAIGU_EVENTS_DIR','PAIGU_SETTLEMENT_DIR','PAIGU_SNAPSHOT_DIR','PAIGU_MEMBERS_SEED_PATH','PAIGU_UI_CAPTURE_DIR','TMP','TEMP','TMPDIR'])assert.ok(env[key].startsWith(directory+path.sep),key);
  assert.equal(env.PAIGU_E2E_FILTER,undefined);assert.equal(env.DEEPSEEK_API_KEY,undefined);assert.equal(env.PAIGU_TEST_KEY_UNSET,undefined);
  assert.equal(env.PAIGU_WEB_DIR,path.join(repo,'web'));assert.equal(env.CARGO_TARGET_DIR,path.join(repo,'target'));assert.equal(env.PATH,'public-path');
  assert.equal(original.PAIGU_CONFIG_PATH,'unwanted');assert.equal(original.PAIGU_E2E_FILTER,'subset');
  assert.equal(original.PAIGU_SNAPSHOT_DIR,'unwanted');
});
test('Cargo JSON selects actual test binaries, not build scripts, and fails closed',()=>{
  const target=path.resolve('target'),exe=path.join(target,'debug','deps','tests.exe');
  const record={reason:'compiler-artifact',profile:{test:true},target:{name:'paigu-bot-core'},executable:exe};
  const text=[{reason:'build-finished',success:true},{...record,profile:{test:false}},record,record].map(v=>JSON.stringify(v)).join('\n');
  assert.deepEqual(testExecutables(text,target),[{executable:exe,target:'paigu-bot-core'}]);
  assert.throws(()=>testExecutables('{}',target),/did not report/);
  assert.throws(()=>testExecutables('invalid-json',target));
  assert.throws(()=>testExecutables(JSON.stringify({...record,executable:path.resolve('outside.exe')}),target),/outside/);
});
test('cleanup accepts only the exact owned temporary child',()=>{
  const parent=path.resolve('temporary');
  assert.equal(managedDirectory(path.join(parent,'paigu-check-owned'),parent,'paigu-check-'),path.join(parent,'paigu-check-owned'));
  for(const value of [parent,path.join(parent,'other'),path.resolve('unrelated','paigu-check-owned')])assert.throws(()=>managedDirectory(value,parent,'paigu-check-'),/unsafe/);
});
test('failed check stops later gates instead of reporting partial success',async()=>{
  const seen=[];
  await assert.rejects(runSteps(['first','fail','never'],async name=>{seen.push(name);if(name==='fail')throw new Error('controlled failure');}),/controlled/);
  assert.deepEqual(seen,['first','fail']);
});
test('real CLI rejects missing explicit private inputs before builds without disclosing the path',()=>{
  const script=fileURLToPath(new URL('../scripts/check.mjs',import.meta.url));
  const missing=path.join(os.tmpdir(),'paigu-nonexistent-'+crypto.randomUUID());
  for(const flag of ['--samples','--settlement-fixture']){
    const result=spawnSync(process.execPath,[script,flag,missing],{encoding:'utf8',timeout:10000,windowsHide:true});
    assert.equal(result.status,1);assert.match(result.stderr,/CHECK FAILED/);
    assert.ok(!result.stderr.includes(missing));assert.ok(!result.stdout.includes('CHECK Rust'));
    assert.ok(!result.stdout.includes('PASS'));
  }
});

test('captured Cargo failure shows bounded rendered errors, not raw artifact or private-looking payloads',()=>{
  const records=[{reason:'compiler-artifact',arbitrary:'not-a-diagnostic'},
    {reason:'compiler-message',message:{level:'warning',rendered:'not-the-failure'}},
    {reason:'compiler-message',message:{level:'error',rendered:'error: expected controlled link failure\n'}},
    {reason:'compiler-message',message:{level:'failure-note',rendered:'note: controlled detail\n'}}];
  assert.equal(cargoDiagnostics('not-json\n'+records.map(v=>JSON.stringify(v)).join('\n')),'error: expected controlled link failure\nnote: controlled detail\n');
  assert.equal(cargoDiagnostics(JSON.stringify({reason:'compiler-message',message:{level:'error',rendered:12}})),'');
  const large=cargoDiagnostics(JSON.stringify({reason:'compiler-message',message:{level:'error',rendered:'x'.repeat(20000)}}));
  assert.ok(large.length<16500);assert.match(large,/truncated/);
});

test('real check stops on test-link failure and displays captured Cargo diagnostics',()=>{
  const parent=os.tmpdir(),root=fs.mkdtempSync(path.join(parent,'paigu-check-link-failure-'));
  const scripts=path.join(root,'scripts'),source=path.join(root,'src');
  const env=isolatedEnvironment(root,root,process.env);
  try{
    fs.mkdirSync(scripts,{recursive:true});fs.mkdirSync(source);fs.mkdirSync(path.join(root,'temp'));
    for(const name of ['check.mjs','check-lib.mjs'])fs.copyFileSync(fileURLToPath(new URL('../scripts/'+name,import.meta.url)),path.join(scripts,name));
    fs.writeFileSync(path.join(root,'Cargo.toml'),'[package]\nname = "paigu-bot-core"\nversion = "0.0.0"\nedition = "2021"\n');
    fs.writeFileSync(path.join(source,'main.rs'),'fn main() {}\n#[cfg(test)]\nmod tests {\n    extern "C" { fn paigu_check_expected_missing_link_symbol(); }\n    #[test]\n    fn link_failure() { unsafe { paigu_check_expected_missing_link_symbol(); } }\n}\n');
    fs.writeFileSync(path.join(root,'config.example.json'),JSON.stringify({llm:{api_key_env:'PAIGU_MISSING_TEST_KEY'},gateway:{},members:{}}));
    const reference=path.join(root,REFERENCE_PATH);fs.mkdirSync(path.dirname(reference),{recursive:true});fs.writeFileSync(reference,'{}');
    const format=spawnSync('cargo',['fmt'],{cwd:root,env,encoding:'utf8',timeout:30000,windowsHide:true});
    assert.equal(format.status,0,format.stderr);
    const result=spawnSync(process.execPath,[path.join(scripts,'check.mjs')],{cwd:root,env,encoding:'utf8',timeout:120000,windowsHide:true,maxBuffer:4*1024*1024});
    assert.equal(result.status,1,result.stderr);
    assert.match(result.stdout,/CHECK Compile Rust tests/);
    assert.match(result.stderr,/paigu_check_expected_missing_link_symbol/);
    assert.match(result.stderr,/Compile Rust tests failed; later checks were not run/);
    assert.ok(!result.stdout.includes('CHECK Rust core')&&!result.stdout.includes('CHECK Public offline CLI')&&!result.stdout.includes('PASS all requested'));
  }finally{fs.rmSync(managedDirectory(root,parent,'paigu-check-link-failure-'),{recursive:true,force:true});}
});
