import path from 'node:path';

export const REFERENCE_TEST='settlement::tests::moonlit_spec_update_fixture_matches_reference';
export const REFERENCE_PATH=path.join('simulation-corpus','real-xlsx','月行水上-更新','fixture.json');
export function options(argv,cwd=process.cwd()){
  const result={ui:false,xlsx:false,help:false,samples:null,settlementFixture:null};
  for(let i=0;i<argv.length;i++){
    const arg=argv[i];
    if(arg==='--help'){result.help=true;continue;}
    if(arg==='--ui'){result.ui=true;continue;}
    if(arg==='--xlsx'){result.xlsx=true;continue;}
    if(arg==='--samples'||arg==='--settlement-fixture'){
      const value=argv[++i];if(!value||value.startsWith('--'))throw new Error(arg+' requires an explicit input path');
      result[arg==='--samples'?'samples':'settlementFixture']=path.resolve(cwd,value);continue;
    }
    throw new Error('Unknown check option: '+arg);
  }
  return result;
}

export function isolatedEnvironment(directory,repository,inherited={}){
  const env={...inherited},store=path.join(directory,'runtime');
  for(const [key,value] of Object.entries({
    PAIGU_CONFIG_PATH:path.join(store,'config','app.json'),PAIGU_ROUNDS_DIR:path.join(store,'rounds'),
    PAIGU_MESSAGES_DIR:path.join(store,'messages'),PAIGU_EVENTS_DIR:path.join(store,'events'),
    PAIGU_SETTLEMENT_DIR:path.join(store,'settlement'),PAIGU_SNAPSHOT_DIR:path.join(store,'snapshots'),PAIGU_COMPUTED_DIR:path.join(store,'computed'),PAIGU_MEMBERS_SEED_PATH:path.join(store,'missing.seed.json'),
    PAIGU_MEMBERS_EXAMPLE_PATH:path.join(repository,'data','members.example.json'),PAIGU_WEB_DIR:path.join(repository,'web'),
    PAIGU_UI_CAPTURE_DIR:path.join(directory,'ui-captures'),CARGO_TARGET_DIR:path.join(repository,'target'),
    TMP:path.join(directory,'temp'),TEMP:path.join(directory,'temp'),TMPDIR:path.join(directory,'temp'),RUST_LOG:'warn'
  }))env[key]=value;
  // An inherited selective test filter must never turn a full check green.
  delete env.PAIGU_E2E_FILTER;
  delete env.PAIGU_TEST_KEY_UNSET;
  delete env.DEEPSEEK_API_KEY;
  delete env.OPENAI_API_KEY;
  return env;
}

export function testExecutables(output,targetDirectory){
  const found=new Map();
  for(const line of output.split(/\r?\n/).filter(s=>s.trim())){
    const record=JSON.parse(line);
    if(record.reason!=='compiler-artifact'||!record.profile?.test||!record.executable)continue;
    const relative=path.relative(targetDirectory,record.executable);
    if(!path.isAbsolute(record.executable)||!relative||relative.startsWith('..')||path.isAbsolute(relative))throw new Error('Test artifact is outside the isolated build target');
    found.set(record.executable,{executable:record.executable,target:record.target?.name});
  }
  if(!found.size)throw new Error('Cargo did not report any test executables');
  return [...found.values()];
}

export function managedDirectory(directory,parent,prefix){
  const resolved=path.resolve(directory);
  if(path.dirname(resolved)!==path.resolve(parent)||!path.basename(resolved).startsWith(prefix))throw new Error('Refusing unsafe temporary-directory cleanup');
  return resolved;
}

export async function runSteps(steps,execute){
  for(const step of steps)await execute(step);
}

export function cargoDiagnostics(output){
  const limit=16*1024,parts=[];let length=0;
  for(const line of output.split(/\r?\n/)){
    let record;try{record=JSON.parse(line);}catch{continue;}
    const message=record?.reason==='compiler-message'?record.message:null;
    if(!['error','failure-note'].includes(message?.level)||typeof message.rendered!=='string')continue;
    const remaining=limit-length;
    if(message.rendered.length>remaining){parts.push(message.rendered.slice(0,remaining),'\n[compiler diagnostics truncated]\n');break;}
    parts.push(message.rendered);length+=message.rendered.length;
  }
  return parts.join('');
}
