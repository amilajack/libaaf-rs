// Local-only real-file verification. Nothing from Downloads is copied into the repo.
import { readdir, readFile, stat, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import init, { AafDocument } from '../../crates/libaaf-wasm/pkg/libaaf_wasm.js';
await init({ module_or_path: await readFile(new URL('../../crates/libaaf-wasm/pkg/libaaf_wasm_bg.wasm', import.meta.url)) });
const directory=process.argv[2]; if (!directory) throw new Error('usage: node tests/downloads.mjs <Downloads directory>');
async function discover(dir) { const files=[]; for (const e of await readdir(dir,{withFileTypes:true})) { const p=path.join(dir,e.name); if(e.isDirectory())files.push(...await discover(p));else if(e.isFile() && /\.aaf$/i.test(e.name))files.push(p); } return files.sort(); }
const report=[];
for(const file of await discover(directory)) {
 const bytes=await readFile(file); const hash=createHash('sha256').update(bytes).digest('hex');
 const d=new AafDocument(bytes,false,false);
 try { const s=JSON.parse(d.inspect()); const tracks=s.tracks.length; const clips=s.tracks.reduce((n,t)=>n+t.items.filter(i=>i.kind==='clip').length,0);
 const reference=spawnSync(new URL('../../tooling/reference-build/bin/aaftool',import.meta.url).pathname,['--aaf-clips','--verb','0','--no-color',file],{encoding:'utf8',maxBuffer:32*1024*1024});
 assert.equal(reference.status,0,file); assert.equal(tracks,(reference.stdout.match(/ (?:Audio|Video)Track\[/g)||[]).length,file);assert.equal(clips,(reference.stdout.match(/ Clip \(/g)||[]).length,file);
 assert.equal(createHash('sha256').update(await readFile(file)).digest('hex'),hash);
 report.push({file:path.basename(file),bytes:(await stat(file)).size,tracks,clips,essences:s.essences.length,objects:s.objectCount,sha256:hash,pass:true});console.log(`PASS ${path.basename(file)}: ${tracks} tracks, ${clips} clips, ${s.essences.length} sources`);
 }finally{d.free();}
}
await mkdir(new URL('../../tooling/results/downloads/',import.meta.url),{recursive:true}); await writeFile(new URL('../../tooling/results/downloads/wasm.json',import.meta.url),JSON.stringify(report,null,2)+'\n');assert.ok(report.length>0);console.log(`${report.length}/${report.length} Downloads AAFs pass WebAssembly import and C track/clip counts.`);
