# libaaf-wasm

WebAssembly bindings for the native, GPL-2.0-or-later `libaaf` Rust reader. The crate exports an owned `AafDocument` through wasm-bindgen. It includes no C runtime.

```sh
wasm-pack build crates/libaaf-wasm --target web --out-dir pkg --release
```

Use the generated ES module and TypeScript declarations:

```js
import init, { AafDocument } from './pkg/libaaf_wasm.js';
await init();
const bytes = new Uint8Array(await file.arrayBuffer());
const document = new AafDocument(bytes, false, false);
try {
  const session = JSON.parse(document.inspect());
  const objects = JSON.parse(document.objects());
  const firstObject = JSON.parse(document.object(objects[0].id));
  const embedded = session.essences.find(e => e.embedded);
  if (embedded) {
    const wavBytes = document.extract_essence(embedded.id, 'wav');
    // Use a Blob or write the Uint8Array in your host application.
  }
} finally {
  document.free();
}
```

Constructor booleans enable ProTools rendered-fade recovery and sample-accurate-edit removal. `inspect()` returns JSON containing producer details, timecode, composition metadata, tracks, clips/transitions, curves, markers, essence descriptors, and diagnostics. `objects()` lists stable object IDs, paths and class names. `object(id)` returns property IDs, stored forms, original bytes and resolved reference IDs.

`extract_essence(index, format)` accepts `original`, `wav`, and `bwav`. `extract_clip(trackIndex, itemIndex)` returns WAV bytes for a single-source embedded clip. Track/item indexes refer to the arrays in `inspect()`, including transition items. External media references cannot be extracted without access to those files; `extract_clip_source(trackIndex, itemIndex, sourceIndex, format)` extracts one trimmed source from a multichannel clip. `extract_range(index, format, offset, length)` accepts exact frame counts as JavaScript `bigint`; zero length produces an empty WAV payload. Invalid input and missing media throw JavaScript `Error` objects. Bounds are checked before writing extraction data. Inspection rejects integers beyond JavaScript’s safe integer range.

The constructor copies a host Uint8Array into owned wasm storage once. Its Rust entry point takes ownership of the generated binding’s `Vec<u8>`, avoiding a second complete input allocation. Native extraction streams to a writer; browser extraction creates a returned Uint8Array because that is the JavaScript API boundary. Always free documents after use. The test app transfers input/output buffers to a worker to keep the UI responsive.

The pinned C reference has an AIFF-to-WAV byte-selection quirk. The port intentionally reproduces it to match upstream's extraction hashes; original AIFF extraction preserves the complete original container. See `media.rs` and the hash tests.

Build and test the app from the repository root:

```sh
npm ci --prefix web
npm run build --prefix web
npm test --prefix web
npm run preview --prefix web
```

All 65 pinned fixtures import through wasm. Six extraction hashes match upstream. Local Downloads verification also passed five additional real AAF files, with C-equivalent track and clip counts. Private input files are not included in the repository.

Inspection includes each essence’s edit rate, source slot, master mob, and origin. Curve points retain the numeric `time`/`value` fields and expose exact `timeRational`/`valueRational` numerator/denominator fields.
