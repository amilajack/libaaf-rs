import { mkdir, copyFile } from 'node:fs/promises';
const fixtures = ['PR_WAV_Internal.aaf', 'MC_Markers.aaf', 'DR_Multichannel_7.1_single_source.aaf'];
await mkdir(new URL('../public/fixtures/', import.meta.url), { recursive: true });
for (const name of fixtures) await copyFile(new URL(`../../tooling/reference/test/aaf/${name}`, import.meta.url), new URL(`../public/fixtures/${name}`, import.meta.url));
