# Verification record

2026-10-06, Apple M5, macOS 27.0.1, Rust 1.99.0. The complete native port is not yet accepted: see features.md and benchmarks.md for outstanding CLI fidelity and memory work.

- All 65 upstream AAF fixtures parse and produce timelines. Every stream also matches between native file-backed and owned-buffer loading, including full copies and trimmed ranges.
- Native cache tests cover boundary crossings, large direct reads, concurrent readers, truncation errors and overflowing offsets. Malformed files also exercise native loading through a Unicode temporary path.
- 71 upstream comparisons pass: 65 expected analyses and six extraction hashes.
- 455 additional native differential cases pass.
- 15 CLI help/required-option/validation/file-selection cases pass, including exact help text.
- 65 stable property/reference comparisons pass with the approved debug-pointer exception.
- All 65 fixtures import through wasm; six extraction hashes and structured-error cases pass.
- Five nested Downloads AAFs pass native comparisons and wasm track/clip-count comparisons, covering 768 tracks and 1,752 clips. Input SHA-256 values are unchanged. Private files and local result reports are excluded from Git.
- Formatting, Clippy with warnings denied, tests, docs, fuzz target compilation, and the web production build pass locally.
- Native Linux/macOS/Windows CI passed at f3445af. The complete Linux reference/wasm job and all three native jobs passed at 6e3402a after fixing the platform-specific archive path.

## Fuzzing

Used cargo-fuzz 0.13.2 and nightly-2026-09-01 (development only), with AddressSanitizer. Each run used `-max_total_time=60 -max_len=1048576 -timeout=5` and completed in 61 seconds without a crash:

| Target | Executions | Seeds |
|---|---:|---|
| document parsing and raw properties | 32,297 | Complete pinned AAF fixture suite |
| document, ProTools recovery and text inspection | 44,143 | AAF fixtures and generated corpus from the first run |
| media/URI parsing | 8,966,293 | Pinned WAV/AIFF files |

After file-backed storage, streamed directory loading and metadata compaction, another document/ProTools/inspection AddressSanitizer campaign completed 31,858 runs in 61 seconds without a crash. The fuzzer uses owned buffers; the native file cache is covered by the focused tests above.

These short campaigns supplement the regression tests; they are not evidence that every malformed input is covered. Generated corpora, logs and artifacts remain in ignored local directories.

```sh
cargo +nightly-2026-09-01 fuzz run document fuzz/corpus/document -- -max_total_time=60 -max_len=1048576 -timeout=5
cargo +nightly-2026-09-01 fuzz run media fuzz/corpus/media -- -max_total_time=60 -max_len=1048576 -timeout=5
```

## Shared metadata storage verification

The shared metadata arena, pooled directory names, and allocation-free contiguous stream descriptors pass the canonical `python3 tooling/verify.py all` suite. Added regression tests reject oversized metadata before allocation and preserve the full width of packed record fields. The native API exposes borrowed object/property/node views; the wasm JSON interface is unchanged.

The rebuilt wasm package and native CLI both pass all five recursively discovered Downloads AAFs (768 tracks, 1,752 clips), with input hashes unchanged. Chrome also rendered the embedded-audio example using this build. A fresh AddressSanitizer document/ProTools/inspection campaign completed 50,612 executions in 61 seconds without a crash.

Version-controlled pre-commit hooks invoke the canonical CI suite without rewriting files. The hook is installed locally, and an intentionally unformatted staged change was rejected before a commit could be created.
