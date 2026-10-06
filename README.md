# libaaf-rs

Native Rust AAF reading, timeline inspection, and embedded audio extraction, with WebAssembly bindings and a local import test app. Based on [LibAAF](https://github.com/agfline/LibAAF) at `7925dff4026be06b4edfacdfe458887c2d6503ca`; GPL-2.0-or-later. Production code runs entirely in Rust. The C submodule is a development reference and supplies the complete upstream fixtures.

The library passes the pinned upstream analyses, extraction hashes, and additional differential checks. The user accepted current performance on 2026-10-06 and excluded further CLI presentation parity work. Historical benchmark gate failures remain recorded. See [the feature checklist](docs/features.md), [verification record](docs/verification.md), and [measured benchmarks](docs/benchmarks.md) for coverage and remaining limitations.

## Browser import app

```sh
git submodule update --init --recursive
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
npm ci --prefix web
npm run dev --prefix web
```

Open the Vite URL, choose or drop an AAF, or try one of the three pinned examples. The app displays tracks, clips, markers, metadata, source references, and raw object properties. Embedded essences and single-source clips can be downloaded; inspection can be exported as JSON. Parsing runs in a web worker, and files stay on the device. External-media paths are displayed as references; the browser does not resolve filesystem paths.

[Bindings and JavaScript usage](crates/libaaf-wasm/README.md). [Rendered import proof](docs/screenshots/import-lab.png).

## Native API

Rust stable was checked on 2026-10-06 and pinned to **1.99.0**, edition 2024.

```rust,no_run
use libaaf::{Document, ReaderOptions};
use libaaf::media::{extract_to_file, ExtractFormat};
use std::path::Path;

let document = Document::open("session.aaf", ReaderOptions::default())?;
let composition = document.composition()?;
for track in &composition.tracks {
    println!("{:?}: {} timeline items", track.name, track.items.len());
}
// Choose an embedded essence by its stable composition index.
extract_to_file(&document, &composition, 0, ExtractFormat::Wave,
    None, Path::new("audio.wav"))?;
# Ok::<(), libaaf::Error>(())
```

`Document::from_bytes` owns the input buffer. Objects use stable IDs and expose original property bytes and indexed references. Unknown property bytes remain inspectable. Native `Document::open` keeps an owned file handle with a bounded read cache; it does not load the whole file. Keep that file unchanged while the document is open, or use `from_bytes` for an immutable snapshot. Media streams read ranges from either backing; extraction streams into a writer without a second embedded-media allocation. Stream `to_vec`, `prefix`, and chunk reads are fallible so later file I/O errors propagate. `ReaderOptions` controls external media location, ProTools fade recovery, sample-accurate-edit removal, and MobID filenames. Failures are structured `Error` values; recoverable interpretation issues appear in document/composition diagnostics.

```sh
cargo run --release -p aaftool -- --aaf-summary --aaf-clips --aaf-essences --no-color session.aaf
cargo run --release -p aaftool -- --extract-essences --extract-path ./output session.aaf
```

## Verification

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
cargo check --manifest-path fuzz/Cargo.toml --locked
npm run build --prefix web
npm test --prefix web
```

Build the development reference, then compare it:

```sh
cmake -S tooling/reference -B tooling/reference-build -DCMAKE_BUILD_TYPE=Release -DBUILD_DOC=OFF -DBUILD_UNIT_TEST=OFF -DBUILD_SHARED_LIB=OFF -DBUILD_STATIC_LIB=ON
cmake --build tooling/reference-build --parallel 8
cargo build --release -p aaftool -p libaaf-acceptance
python3 tooling/upstream_tests.py
python3 tooling/differential.py
python3 tooling/cli_differential.py
python3 tooling/build_reference_tools.py
python3 tooling/inspect_differential.py
```

`tooling/fixtures.json` records upstream fixture paths, provenance, file sizes, and SHA-256 checksums. The complete `test` tree remains at its original relative paths in the pinned submodule. `.github/workflows/ci.yml` runs formatting, Clippy, tests and docs on Linux, macOS and Windows, and reference/wasm comparisons on Linux. CI timings are not performance acceptance evidence.

To test your own nested files locally, after building the reference and wasm package:

```sh
python3 tooling/test_downloads.py /path/to/Downloads
node web/tests/downloads.mjs /path/to/Downloads
```

These commands do not copy input files into the repository. Reports go to ignored `tooling/results`. Five AAF files nested in Downloads passed native comparisons and wasm track/clip count checks on this Mac; 768 tracks and 1,752 clips in total. The originals' SHA-256 checksums were unchanged.

Fuzz targets are in `fuzz/`. Run with cargo-fuzz on a nightly development toolchain, using an ignored writable corpus and the pinned fixtures as read-only seeds:

```sh
mkdir -p fuzz/corpus/document
cargo +nightly fuzz run document fuzz/corpus/document tooling/reference/test/aaf -- -max_total_time=300
```
 Nightly is used only by fuzz tooling, never by production crates.

No crates.io publication or public release is configured.

### File-backed reader API changes

`CompoundFile::sector` returns `Cow<[u8]>`. `Stream::to_vec` and `prefix` now return `Result<Vec<u8>>`; propagate failures with `?`. `chunks` yields fallible `Cow<[u8]>` values in bounded pieces. Use `reader` or `copy_to` for streaming extraction. `CompoundFile::nodes.get()` and `.iter()` return borrowed node views; `Node::name` borrows from a shared name buffer. Metadata records and original bytes share document-owned arrays. Use `document.objects()` and `object.properties()` to iterate borrowed views; `object`, `get_object`, and `property` return views by value. `Property::references` is a borrowed slice. `PropertyDefinition::name` is `Cow<'static, str>`; use `.as_ref()` for a string slice or `.into_owned()` when ownership is needed. Views and their raw bytes cannot outlive the document. Stable IDs and the wasm JSON interface are unchanged. Native files must remain unchanged while open; `Document::from_bytes` retains snapshot semantics.

### Required local verification hooks

Run `python3 tooling/install_hooks.py` after cloning. The versioned pre-commit hook runs `python3 tooling/verify.py all --staged`: the same formatting, Clippy/type checking, tests, documentation, release build, C differential/extraction checks, fuzz compilation, and wasm/web build/tests used in CI. Install the pinned Rust/wasm toolchain, CMake, a C compiler, Node/npm, and wasm-pack 0.15.0 first. Missing tools or failed checks block the commit. Stage all intended files and keep tracked files synchronized with the index so verification covers the proposed commit. Checks never auto-format source files or bypass failures. The installer preserves any existing hook setup by refusing to replace it.
