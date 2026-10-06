import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import init, { AafDocument } from '../../crates/libaaf-wasm/pkg/libaaf_wasm.js';
await init({ module_or_path: await readFile(new URL('../../crates/libaaf-wasm/pkg/libaaf_wasm_bg.wasm', import.meta.url)) });
const fixtures = new URL('../../tooling/reference/test/aaf/', import.meta.url);
const read = name => readFile(new URL(name, fixtures));
test('all 65 pinned fixtures import through WebAssembly and expose structured timelines', async () => {
  let count = 0;
  for (const name of (await readdir(fixtures)).filter(n => n.endsWith('.aaf')).sort()) {
    const d = new AafDocument(await read(name), false, false);
    try {
      const data = JSON.parse(d.inspect());
      assert.ok(data.objectCount > 0, name);
      assert.ok(data.name, name);
      // Count comparisons use a fresh C reference run for each options combination
      // in tooling/differential.py; this smoke test verifies every browser import.
      assert.ok(Array.isArray(data.tracks));
      assert.ok(Array.isArray(data.essences));
      count++;
    } finally { d.free(); }
  }
  assert.equal(count, 65);
});
test('embedded extraction reproduces upstream WAV, AIFF, and PCM hashes', async () => {
  const cases = [
    ['PR_WAV_Internal.aaf', 'original', 'b49538965723bb1840e01b6710da20b8'],
    ['PR_AIFF_Internal.aaf', 'original', '694634f1af77e1c23e76b0b41d2b223f'],
    ['PT_PCM_Internal.aaf', 'wav', '2a8f46cf946e44973a4a73f84504a4c5'],
  ];
  for (const [name, format, hash] of cases) {
    const d = new AafDocument(await read(name), false, false);
    try { const bytes = d.extract_essence(0, format); assert.equal(createHash('md5').update(bytes).digest('hex'), hash, name); }
    finally { d.free(); }
  }
  for (const [name, hash] of [['PR_WAV_Internal.aaf', '3089dbaa3d9e03b820695504d5334d7c'], ['PR_AIFF_Internal.aaf', '44b9acf682cb12fa9c692f4e3c591079'], ['PT_PCM_Internal.aaf', '2a8f46cf946e44973a4a73f84504a4c5']]) {
    const d = new AafDocument(await read(name), false, false);
    try { const data = JSON.parse(d.inspect()); const ti = data.tracks.findIndex(t => !t.video && t.items.some(i => i.kind === 'clip')); const ci = data.tracks[ti].items.findIndex(i => i.kind === 'clip'); assert.equal(createHash('md5').update(d.extract_clip(ti, ci)).digest('hex'), hash, name); }
    finally { d.free(); }
  }
});
test('structured malformed-file error and object inspection', async () => {
  assert.throws(() => new AafDocument(new Uint8Array(512), false, false), /CFB signature/);
  const d = new AafDocument(await read('PR_WAV_Internal.aaf'), false, false);
  try { const objects = JSON.parse(d.objects()); assert.ok(objects.length > 0); const object = JSON.parse(d.object(objects[0].id)); assert.ok(object.properties.length > 0); assert.throws(() => d.object(0xffffffff), /invalid object ID/); assert.throws(() => d.extract_essence(0xffffffff, 'wav'), /essence index/); }
  finally { d.free(); }
});
test('clip extraction rejects invalid indices and multiple source files', async () => {
  const d = new AafDocument(await read('DR_Multichannel_7.1_single_source.aaf'), false, false);
  try {
    const data = JSON.parse(d.inspect());
    const ti = data.tracks.findIndex(t => t.items.some(i => i.kind === 'clip' && i.sources.length > 1));
    assert.ok(ti >= 0);
    const ci = data.tracks[ti].items.findIndex(i => i.kind === 'clip');
    assert.throws(() => d.extract_clip(ti, ci), /single media source/);
    assert.throws(() => d.extract_clip(0xffffffff, 0), /invalid track index/);
    assert.throws(() => d.extract_clip(ti, 0xffffffff), /invalid clip index/);
    assert.throws(() => d.extract_essence(0, 'mp3'), /format must be/);
  } finally { d.free(); }
});

test('exact frame ranges, clip sources, and rational media metadata', async () => {
  const d = new AafDocument(await read('PT_PCM_Internal.aaf'), false, false);
  try {
    const data = JSON.parse(d.inspect());
    const essence = data.essences[0];
    assert.equal(essence.editRate.numerator, essence.sampleRate);
    assert.equal(essence.editRate.denominator, 1);
    assert.ok(Number.isInteger(essence.sourceSlot));
    const ti = data.tracks.findIndex(t => t.items.some(i => i.kind === 'clip'));
    const ci = data.tracks[ti].items.findIndex(i => i.kind === 'clip');
    assert.deepEqual(d.extract_clip_source(ti, ci, 0, 'wav'), d.extract_clip(ti, ci));
    assert.throws(() => d.extract_clip_source(ti, ci, 0xffffffff, 'wav'), /invalid source index/);
    const whole = d.extract_essence(0, 'wav');
    const range = d.extract_range(0, 'wav', 1n, 17n);
    const block = essence.channels * essence.sampleSize / 8;
    assert.equal(range.length, 44 + 17 * block);
    assert.deepEqual(range.slice(44), whole.slice(44 + block, 44 + 18 * block));
    assert.throws(() => d.extract_range(0, 'wav', 0xffffffffffffffffn, 1n), /range|offset/);
    assert.equal(d.extract_range(0, 'wav', 0n, 0n).length, 44);
  } finally { d.free(); }
});
