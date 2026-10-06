import init, { AafDocument } from '../../crates/libaaf-wasm/pkg/libaaf_wasm.js';
import wasmUrl from '../../crates/libaaf-wasm/pkg/libaaf_wasm_bg.wasm?url';
const ready = init({ module_or_path: wasmUrl });
let document;
self.onmessage = async ({ data }) => {
  const { id, action, args } = data;
  try {
    await ready;
    let result;
    if (action === 'open') {
      const next = new AafDocument(new Uint8Array(args.bytes), args.recoverFades, args.removeSae);
      try { result = JSON.parse(next.inspect()); } catch (error) { next.free(); throw error; }
      document?.free(); document = next;
    } else if (action === 'ready') result = true;
    else if (!document) throw new Error('Import an AAF first.');
    else if (action === 'objects') result = JSON.parse(document.objects());
    else if (action === 'object') result = JSON.parse(document.object(args.id));
    else if (action === 'essence') result = document.extract_essence(args.index, args.format);
    else if (action === 'clip') result = document.extract_clip(args.track, args.item);
    else throw new Error('Unknown reader action.');
    if (result instanceof Uint8Array) self.postMessage({ id, result }, [result.buffer]);
    else self.postMessage({ id, result });
  } catch (error) { self.postMessage({ id, error: error instanceof Error ? error.message : String(error) }); }
};
