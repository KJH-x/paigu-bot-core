import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import os from 'node:os';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {options,isolatedEnvironment,testExecutables,managedDirectory,runSteps} from '../scripts/check-lib.mjs';

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
  const original={PATH:'public-path',PAIGU_CONFIG_PATH:'unwanted',PAIGU_ROUNDS_DIR:'unwanted',PAIGU_UI_CAPTURE_DIR:'unwanted',PAIGU_E2E_FILTER:'subset',DEEPSEEK_API_KEY:'not-real',PAIGU_TEST_KEY_UNSET:'not-real'};
  const env=isolatedEnvironment(directory,repo,original);
  for(const key of ['PAIGU_CONFIG_PATH','PAIGU_ROUNDS_DIR','PAIGU_MESSAGES_DIR','PAIGU_EVENTS_DIR','PAIGU_SETTLEMENT_DIR','PAIGU_MEMBERS_SEED_PATH','PAIGU_UI_CAPTURE_DIR','TMP','TEMP','TMPDIR'])assert.ok(env[key].startsWith(directory+path.sep),key);
  assert.equal(env.PAIGU_E2E_FILTER,undefined);assert.equal(env.DEEPSEEK_API_KEY,undefined);assert.equal(env.PAIGU_TEST_KEY_UNSET,undefined);
  assert.equal(env.PAIGU_WEB_DIR,path.join(repo,'web'));assert.equal(env.CARGO_TARGET_DIR,path.join(repo,'target'));assert.equal(env.PATH,'public-path');
  assert.equal(original.PAIGU_CONFIG_PATH,'unwanted');assert.equal(original.PAIGU_E2E_FILTER,'subset');
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
