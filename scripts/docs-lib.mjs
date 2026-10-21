import path from 'node:path';

export function localLinks(markdown){
  // This repository uses inline links without whitespace in local targets.
  return [...markdown.matchAll(/\[([^\]\n]+)\]\((\.{1,2}\/[^\s)]+)\)/g)].map(m=>({label:m[1],target:decodeURIComponent(m[2].split('#')[0])}));
}
export function linkPath(file,target,repository){
  const resolved=path.resolve(path.dirname(file),target),relative=path.relative(repository,resolved);
  if(relative.startsWith('..')||path.isAbsolute(relative))throw new Error('Local document link escapes the repository');
  return resolved;
}
export function sourceSymbol(label,filename){
  if(!/\.(rs|js|mjs)$/.test(filename))return null;
  const match=/^`([A-Za-z_][\w]*(?:::[A-Za-z_][\w]*)*)`$/.exec(label);
  return match?match[1].split('::').at(-1):null;
}
export function declares(source,symbol,filename){
  const word=symbol.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
  const pattern=filename.endsWith('.rs')?'\\b(?:fn|struct|enum|trait|type)\\s+'+word+'\\b':'\\bfunction\\s+'+word+'\\s*\\(';
  return new RegExp(pattern).test(source);
}
