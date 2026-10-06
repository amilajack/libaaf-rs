# Feature and acceptance checklist

Reference: `agfline/LibAAF@7925dff4026be06b4edfacdfe458887c2d6503ca`. Checkmarks identify implemented/verified behavior, not overall port completion. Writing and expanded video support are outside scope.

## Storage and AAF core

- [x] CFB v3/v4 headers, DIFAT/FAT/MiniFAT, directory tree, mini streams and regular streams
- [x] Checked offsets, lengths and allocation-table bounds; directory/allocation cycles fail
- [x] Standard definitions generated from pinned headers and standard class/property registration
- [x] Metadata dictionary, custom classes/properties, inheritance, original unknown property bytes
- [x] Stable contiguous object arena and indexed strong-reference sets/vectors and weak references
- [x] Typed strings, identifiers, integers, rationals, indirect values, tagged metadata and raw inspection
- [x] Deterministic raw stream/property inspection with borrowed stream ranges
- [x] Original property bytes and resolved strong-reference edges match C on all 65 fixtures
- [ ] Complete upstream parsed-property/debugging presentation equivalence
- [x] AddressSanitizer fuzz runs for document parsing, text inspection/ProTools recovery, and media/URI parsing (see verification.md)

## Timeline and application behavior

- [x] Composition selection and usage codes; sequence, filler, source clip, selector and nested scope traversal
- [x] Tracks/clips, multichannel combiners, master/source/derivation chains and subclip metadata
- [x] Gain, volume automation, pan and interpolation types
- [x] Transitions/fades, timecode, markers and metadata in upstream analysis comparisons
- [x] Existing single-video-track/single-video-clip interpretation and inspection
- [x] Media Composer clip/track mute/solo, audio warp, adjusted/subclip behavior and marker colors
- [x] ProTools localized rendered fade/SAE names, recovery with sufficient handles, SAE removal
- [x] Resolve multichannel combiner length behavior
- [x] Native/wasm API exposes the pinned reader’s normalized two-point transition curves
- [ ] Remaining uncommon graph combinations audited

## Media and extraction

- [x] URI decoding, external-media common-prefix/path search and Unicode paths
- [x] Embedded PCM, WAV and AIFF inspection/extraction and clip ranges
- [x] WAV/BWAV headers, source time reference, UMID and descriptive metadata
- [x] Upstream analysis expectations and six extraction hashes
- [x] Bounds failures for clips past media end and arithmetic overflow in sample ranges
- [x] Invalid extraction ranges/formats are rejected before writing or creating/truncating a file
- [ ] Every extraction CLI option combination and filesystem I/O failure audited

## CLI

- [x] Main summary/essence/clip options, four position formats, sample-rate override, metadata/automation display
- [x] CFB header, allocation tables, directory paths and class/metadictionary comparisons
- [x] External media location, relative paths, MobID naming and extraction commands implemented
- [x] ProTools options, log-file output, help, required-option validation and exit-255 failures implemented
- [ ] Full trace/dump-class/dump-class-raw/dump-meta/dump-tagged-value output compatibility
- [ ] Colored/default diagnostic output and complete error/exit-message equivalence

## WebAssembly and app

- [x] Separate `libaaf-wasm` crate with wasm-bindgen, generated JavaScript/TypeScript interface
- [x] Worker-based local AAF file/drop import, pinned examples, timeline/media/metadata/object views
- [x] Inspection JSON export and embedded essence/single-source clip download
- [x] Chrome rendered verification for embedded audio, multichannel and marker examples
- [x] Chrome clip download MD5 verified against upstream
- [x] All 65 fixtures pass wasm import; all six extraction hashes pass
- [x] Five nested Downloads files pass native C comparisons and wasm track/clip counts; originals unchanged
- [ ] Automated Chrome file-picker upload proof for private Downloads files (file chooser event did not arrive)

## Acceptance

- [x] Complete pinned upstream test tree, attribution, licensing and fixture SHA-256 manifest
- [x] All 71 upstream comparison/hash cases pass locally
- [x] 455 additional differential cases pass locally, without normalization of substantive output
- [x] Native formatting, Clippy, tests, docs and wasm production build/test run locally
- [x] Linux/macOS/Windows verification workflow configured
- [x] Native CI passed on Linux, macOS and Windows (`f3445af`)
- [ ] Full Linux reference/wasm CI confirmed passing
- [x] Separate paired release parse/extraction benchmark harnesses and measured Mac baseline
- [ ] Every designated workload at most 5% slower than C with adequate confidence
- [ ] Every designated workload peak memory at most 10% above C
- [ ] Controlled final acceptance run, memory profile and remaining optimization

Completion requires all acceptance gates and the remaining fidelity work. Repository creation, the passing main fixture suite and the working web app do not establish complete port parity.

## Approved debug comparison rule

The user approved comparing original property bytes and stable object references instead of C process pointers. `tooling/inspection.c` reads original property streams through C and resolves reference targets through the C object graph. `tooling/inspect_differential.py` compares every byte and reference edge using on-disk directory IDs. It sorts complete records for deterministic comparison and does not discard properties, values, or edges. All 65 fixtures pass. This does not claim identical trace/debug presentation.
