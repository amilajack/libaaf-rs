# Verification record

2026-10-06, Apple M5, macOS 27.0.1, Rust 1.99.0. The complete native port is not yet accepted: see features.md and benchmarks.md for outstanding CLI fidelity and memory work.

- All 65 upstream AAF fixtures parse and produce timelines.
- 71 upstream comparisons pass: 65 expected analyses and six extraction hashes.
- 455 additional native differential cases pass.
- 65 stable property/reference comparisons pass with the approved debug-pointer exception.
- All 65 fixtures import through wasm; six extraction hashes and structured-error cases pass.
- Five nested Downloads AAFs pass native comparisons and wasm track/clip-count comparisons, covering 768 tracks and 1,752 clips. Input SHA-256 values are unchanged. Private files and local result reports are excluded from Git.
- Formatting, Clippy with warnings denied, tests, docs, fuzz target compilation, and the web production build pass locally.
- Native Linux/macOS/Windows CI passed at f3445af. The Linux reference tool's platform-specific archive path was repaired in 6e3402a.

## Fuzzing

Used cargo-fuzz 0.13.2 and nightly-2026-09-01 (development only), with AddressSanitizer. Each run used `-max_total_time=60 -max_len=1048576 -timeout=5` and completed in 61 seconds without a crash:

| Target | Executions | Seeds |
|---|---:|---|
| document parsing and raw properties | 32,297 | Complete pinned AAF fixture suite |
| document, ProTools recovery and text inspection | 44,143 | AAF fixtures and generated corpus from the first run |
| media/URI parsing | 8,966,293 | Pinned WAV/AIFF files |

These short campaigns supplement the regression tests; they are not evidence that every malformed input is covered. Generated corpora, logs and artifacts remain in ignored local directories.

```sh
cargo +nightly-2026-09-01 fuzz run document fuzz/corpus/document -- -max_total_time=60 -max_len=1048576 -timeout=5
cargo +nightly-2026-09-01 fuzz run media fuzz/corpus/media -- -max_total_time=60 -max_len=1048576 -timeout=5
```
