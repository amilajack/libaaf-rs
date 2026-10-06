import './style.css';
const $ = id => document.getElementById(id);
const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
const pending = new Map(); let requestId = 0, session, filename, objects = [], importing = false;
worker.onmessage = ({ data }) => { const p = pending.get(data.id); if (p) { pending.delete(data.id); data.error ? p.reject(new Error(data.error)) : p.resolve(data.result); } };
worker.onerror = error => { for (const p of pending.values()) p.reject(new Error(error.message || 'The AAF reader stopped. Reload to retry.')); pending.clear(); showError(error.message || 'The AAF reader could not start.'); };
function request(action, args = {}, transfer = []) { return new Promise((resolve, reject) => { const id = ++requestId; pending.set(id, { resolve, reject }); worker.postMessage({ id, action, args }, transfer); }); }
function node(tag, text, className) { const e = document.createElement(tag); if (text !== undefined) e.textContent = text; if (className) e.className = className; return e; }
function showError(message) { $('error').textContent = message; $('error').hidden = false; }
function duration(value, rate) { return rate?.value > 0 ? value / rate.value : 0; }
const number = v => new Intl.NumberFormat('en-US', { maximumFractionDigits: 3 }).format(v);
function download(data, name, type = 'application/octet-stream') { const url = URL.createObjectURL(new Blob([data], { type })); const a = node('a'); a.href = url; a.download = name; a.click(); setTimeout(() => URL.revokeObjectURL(url), 3000); }
function downloadName(e) { const n = e.uniqueName.replace(/[\/\\<>:"|?*\x00-\x1f]/g, '_'); if (e.format === 'Aiff') return /\.(aif|aiff|aifc)$/i.test(n) ? n : `${n}.aif`; return /\.(wav|wave)$/i.test(n) ? n : `${n}.wav`; }
async function openFile(file) {
  if (importing) return;
  importing = true; $('session').inert = true; $('error').hidden = true; $('status').textContent = `Reading ${file.name}…`;
  for (const b of document.querySelectorAll('[data-fixture]')) b.disabled = true;
  $('file').disabled = true;
  try {
    const bytes = await file.arrayBuffer(); const start = performance.now();
    const result = await request('open', { bytes, recoverFades: $('recover-fades').checked, removeSae: $('remove-sae').checked }, [bytes]);
    session = result; filename = file.name; objects = []; $('object-list').replaceChildren(); $('object-detail').textContent = 'Select an object to inspect its raw properties.';
    render(); $('status').textContent = `${filename} · ${number(file.size / 1024)} KB · imported in ${Math.round(performance.now() - start)} ms`;
  } catch (error) { showError(`Could not import ${file.name}: ${error.message}`); $('status').textContent = session ? 'Previous session remains open.' : 'Choose another AAF file to try again.'; }
  finally { importing = false; $('session').inert = false; $('file').disabled = false; for (const b of document.querySelectorAll('[data-fixture]')) b.disabled = false; }
}
$('file').addEventListener('change', e => { if (e.target.files[0]) openFile(e.target.files[0]); e.target.value = ''; });
for (const event of ['dragenter', 'dragover']) $('drop-zone').addEventListener(event, e => { e.preventDefault(); $('drop-zone').classList.add('dragging'); });
for (const event of ['dragleave', 'drop']) $('drop-zone').addEventListener(event, e => { e.preventDefault(); $('drop-zone').classList.remove('dragging'); });
$('drop-zone').addEventListener('drop', e => { if (e.dataTransfer.files[0]) openFile(e.dataTransfer.files[0]); });
for (const button of document.querySelectorAll('[data-fixture]')) button.addEventListener('click', async () => { try { const response = await fetch(`./fixtures/${button.dataset.fixture}`); if (!response.ok) throw new Error('Example file unavailable.'); await openFile(new File([await response.blob()], button.dataset.fixture)); } catch (e) { showError(e.message); } });
function render() {
  $('empty').hidden = true; $('session').hidden = false; $('filename').textContent = filename; $('session-name').textContent = session.name || 'Untitled composition';
  $('producer').textContent = [session.producer?.product, session.producer?.version, session.producer?.company].filter(Boolean).join(' · ');
  const stats = [['Tracks', session.tracks.length], ['Clips', session.tracks.reduce((n, t) => n + t.items.filter(i => i.kind === 'clip').length, 0)], ['Sample rate', session.sampleRate ? `${number(session.sampleRate / 1000)} kHz` : 'Not specified'], ['Timecode', `${session.timecode.fps} fps ${session.timecode.drop ? 'DF' : 'NDF'}`], ['Objects', number(session.objectCount)]];
  $('stats').replaceChildren(...stats.map(([label, value]) => { const s = node('div', undefined, 'stat'); s.append(node('span', label), node('strong', value)); return s; }));
  $('track-count').textContent = session.tracks.length; $('media-count').textContent = session.essences.length;
  renderTimeline(); renderMedia(); renderMetadata();
}
function renderTimeline() {
  const list = $('timeline-tracks'); list.replaceChildren(); $('clip-detail').hidden = true;
  if (!session.tracks.length) { list.append(node('p', 'This composition has no audio or video tracks.', 'muted')); return; }
  const end = Math.max(1, ...session.tracks.map(t => duration(t.length, t.editRate)));
  const ruler = node('div', undefined, 'timeline-ruler'); ruler.append(node('div', 'TRACK / FORMAT')); const scale = node('div', undefined, 'ruler-scale'); for (let i = 0; i <= 4; i++) scale.append(node('span', `${number(end * i / 4)}s`)); ruler.append(scale); list.append(ruler);
  session.tracks.forEach((t, ti) => {
    const row = node('div', undefined, 'track-row'); const label = node('div', undefined, 'track-label'); label.append(node('strong', t.name || `${t.video ? 'Video' : 'Audio'} ${t.number}`), node('span', `${t.video ? 'VIDEO' : `${t.channels || '?'} CHANNEL${t.channels === 1 ? '' : 'S'}`} ${t.mute ? ' · MUTED' : ''} ${t.solo ? ' · SOLO' : ''}`)); row.append(label);
    const lane = node('div', undefined, 'track-lane');
    t.items.forEach((clip, ci) => { if (clip.kind !== 'clip') return; const source = session.essences[clip.sources[0]?.essence]; const b = node('button', clip.name || source?.name || 'Unresolved clip', `timeline-clip ${t.video ? 'video-clip' : ''} ${clip.mute ? 'muted-clip' : ''}`); b.style.left = `${Math.max(0, duration(clip.position, t.editRate) / end * 100)}%`; b.style.width = `${Math.max(0.4, duration(clip.length, t.editRate) / end * 100)}%`; b.title = `${b.textContent} · ${number(duration(clip.position, t.editRate))}s · ${number(duration(clip.length, t.editRate))}s long`; b.addEventListener('click', () => clipDetail(ti, ci)); lane.append(b); });
    if (!t.items.some(i => i.kind === 'clip')) lane.append(node('span', 'Empty track', 'empty-track'));
    row.append(lane); list.append(row);
  });
  const markers = $('markers'); markers.replaceChildren(); if (session.markers.length) { markers.append(node('h3', `Markers · ${session.markers.length}`)); session.markers.forEach(m => { const e = node('div', undefined, 'marker'); const dot = node('i'); dot.style.background = `rgb(${m.color.map(v => v > 255 ? v >> 8 : v).join(',')})`; e.append(dot, node('strong', m.name || 'Marker'), node('span', `${number(duration(m.position, m.editRate))}s`), node('p', m.comment || '')); markers.append(e); }); }
}
function clipDetail(ti, ci) {
  const t = session.tracks[ti], c = t.items[ci]; const d = $('clip-detail'); d.hidden = false; d.replaceChildren(node('h3', c.name || session.essences[c.sources[0]?.essence]?.name || 'Clip'));
  d.append(node('p', `Start ${number(duration(c.position, t.editRate))}s · Length ${number(duration(c.length, t.editRate))}s · Source offset ${number(duration(c.sourceOffset, t.editRate))}s · ${c.channels} channels`));
  if (c.gain) d.append(node('p', `Gain: ${c.gain.constant ? `${number(20 * Math.log10(c.gain.points[0].value))} dB` : `${c.gain.points.length} automation points`}`));
  if (c.automation) d.append(node('pre', JSON.stringify(c.automation, null, 2)));
  for (const s of c.sources) d.append(node('p', `${session.essences[s.essence].uniqueName} · ${s.channel ? `channel ${s.channel}` : 'all channels'}`));
  if (c.sources.length === 1 && session.essences[c.sources[0].essence]?.embedded) { const b = node('button', 'Download clip WAV ↓', 'button'); b.addEventListener('click', async () => { try { download(await request('clip', { track: ti, item: ci }), `${t.number}_${ci + 1}_${session.essences[c.sources[0].essence].uniqueName.replace(/\.wav$/i, '')}.wav`, 'audio/wav'); } catch (e) { showError(e.message); } }); d.append(b); }
}
function renderMedia() {
  const list = $('media-list'); list.replaceChildren();
  if (!session.essences.length) list.append(node('p', 'No source media found in this composition.', 'muted'));
  for (const e of session.essences) { const row = node('article', undefined, 'media-row'); const icon = node('div', e.embedded ? '▥' : '↗', 'media-icon'); const text = node('div', undefined, 'media-text'); text.append(node('h4', e.uniqueName), node('p', `${e.format.toUpperCase()} · ${e.channels} ch · ${number(e.sampleRate)} Hz · ${e.sampleSize} bit · ${e.sampleRate ? number(e.length / e.sampleRate) : '?'}s`)); if (e.originalPath) text.append(node('code', e.originalPath)); const badge = node('span', e.embedded ? 'EMBEDDED' : 'EXTERNAL REFERENCE', `badge ${e.embedded ? 'embedded' : ''}`); row.append(icon, text, badge); if (e.embedded) { const b = node('button', 'Download ↓', 'button'); b.addEventListener('click', async () => { try { download(await request('essence', { index: e.id, format: e.format === 'Pcm' ? 'wav' : 'original' }), downloadName(e), e.format === 'Aiff' ? 'audio/aiff' : 'audio/wav'); } catch (error) { showError(error.message); } }); row.append(b); } list.append(row); }
}
function renderMetadata() { const list = $('metadata-list'); list.replaceChildren(); for (const [key, value] of session.metadata) { const row = node('div', undefined, 'metadata-row'); row.append(node('strong', key), node('span', value)); list.append(row); } if (!session.metadata.length) list.append(node('p', 'No composition metadata.', 'muted')); $('diagnostics').replaceChildren(...session.diagnostics.map(d => node('p', `${d.objectId === null ? '' : `Object ${d.objectId} · `}${d.message}`, 'diagnostic'))); if (!session.diagnostics.length) $('diagnostics').append(node('p', 'No parser diagnostics.', 'muted')); }
async function loadObjects() { const current = session; if (!objects.length) { const next = await request('objects'); if (session !== current) return; objects = next; } renderObjects(); }
function renderObjects() { const q = $('object-filter').value.toLowerCase(); const matches = objects.filter(o => `${o.class} ${o.path}`.toLowerCase().includes(q)); const list = $('object-list'); list.replaceChildren(...matches.slice(0, 200).map(o => { const b = node('button', undefined, 'object-row'); b.append(node('strong', `${o.id} · ${o.class}`), node('small', o.path)); b.addEventListener('click', async () => { try { $('object-detail').textContent = JSON.stringify(await request('object', { id: o.id }), null, 2); } catch (e) { showError(e.message); } }); return b; })); if (matches.length > 200) list.append(node('p', `${matches.length} matches. Filter to narrow the first 200 shown.`, 'muted')); }
$('object-filter').addEventListener('input', renderObjects);
const tabs = [...document.querySelectorAll('[role=tab]')];
function selectTab(tab) { for (const b of tabs) { const selected = b === tab; b.setAttribute('aria-selected', String(selected)); b.tabIndex = selected ? 0 : -1; $(b.getAttribute('aria-controls')).hidden = !selected; } if (tab.id === 'tab-objects') loadObjects().catch(e => showError(e.message)); }
tabs.forEach((tab, index) => { tab.addEventListener('click', () => selectTab(tab)); tab.addEventListener('keydown', e => { let target; if (e.key === 'ArrowRight') target = tabs[(index + 1) % tabs.length]; if (e.key === 'ArrowLeft') target = tabs[(index - 1 + tabs.length) % tabs.length]; if (e.key === 'Home') target = tabs[0]; if (e.key === 'End') target = tabs.at(-1); if (target) { e.preventDefault(); target.focus(); selectTab(target); } }); });
$('export-json').addEventListener('click', () => download(JSON.stringify(session, null, 2), `${filename}.json`, 'application/json'));
request('ready').then(() => { $('status').textContent = 'Reader ready · choose a file or try an example'; }).catch(e => showError(e.message));
