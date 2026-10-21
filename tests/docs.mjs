import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {localLinks,linkPath,sourceSymbol,declares} from '../scripts/docs-lib.mjs';

const repository=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
test('local links keep anchors out of filesystem paths and reject escapes',()=>{
  assert.deepEqual(localLinks('[label](../src/main.rs#part) [remote](https://example.com/)'),[{label:'label',target:'../src/main.rs'}]);
  const file=path.join(repository,'docs','fixture.md');
  assert.equal(linkPath(file,'../src/main.rs',repository),path.join(repository,'src','main.rs'));
  assert.throws(()=>linkPath(file,'../../outside',repository),/escapes/);
});
test('explicit source symbols require a declaration, not merely a call',()=>{
  assert.equal(sourceSymbol('`Pipeline::ingest`','mod.rs'),'ingest');
  assert.equal(sourceSymbol('ordinary module label','mod.rs'),null);
  assert.equal(declares('pub async fn ingest(&self) {}','ingest','mod.rs'),true);
  assert.equal(declares('pub struct AllocationEngine {}','AllocationEngine','mod.rs'),true);
  assert.equal(declares('ingest();','ingest','mod.rs'),false);
  assert.equal(declares('function render() {}','render','common.js'),true);
  assert.equal(declares('render();','render','common.js'),false);
});
test('current document links and FUNCTIONAL source symbols resolve',()=>{
  const docs=path.join(repository,'docs');
  const files=fs.readdirSync(docs).filter(name=>name.endsWith('.md')).map(name=>path.join(docs,name));
  files.push(...['README.md','web/README.md','tests/e2e/README.md'].map(name=>path.join(repository,name)));
  let links=0,symbols=0;
  for(const file of files){
    for(const link of localLinks(fs.readFileSync(file,'utf8'))){
      const target=linkPath(file,link.target,repository);
      assert.ok(fs.existsSync(target),path.relative(repository,file)+': broken link '+link.target);links++;
      if(path.basename(file)==='FUNCTIONAL.md'){
        const symbol=sourceSymbol(link.label,target);if(!symbol)continue;
        assert.ok(declares(fs.readFileSync(target,'utf8'),symbol,target),link.target+': missing symbol '+symbol);symbols++;
      }
    }
  }
  assert.ok(links>150,'Document link coverage unexpectedly fell');assert.ok(symbols>=35,'Current functional source symbol coverage unexpectedly fell');
  console.log('PASS '+links+' local links and '+symbols+' explicit functional source symbols');
});
