# Mac performance baseline

This diagnostic baseline does **not** pass the requested acceptance gates. Native Rust uses an owned file buffer and currently exceeds the C +10% peak-memory limit. Normal desktop activity continued during measurement, so a controlled final run is still required.

Both harnesses parse the AAF and interpret its timeline. Parsing measures repeated complete load/parse/drop operations; extraction parses once, then repeatedly extracts every embedded audio essence as WAV into the same temporary directory. C is built with CMake Release and the harness with `-O3 -DNDEBUG`; Rust uses the workspace release profile. Logging is disabled. Neither harness prints analysis output inside timed work. No memory measurements are subtracted from the process baseline.

Each result uses three warm-up batches, 11 alternating paired measurements and 100 operations per batch. The ratio interval is a 95% normal approximation over log paired timing ratios. Memory compares the largest process peak RSS from each implementation across the paired batches. Raw results preserve each timing, RSS and macOS peak footprint.

| Workload | Mode | C median ms | Rust median ms | Rust/C ratio | 95% ratio interval | Peak RSS ratio |
|---|---|---:|---:|---:|---:|---:|
| small (MC_Empty.aaf) | parse | 1.118 | 1.164 | 1.019 | 0.952–1.091 | 1.545 |
| large (PT_Fades.aaf) | parse | 2.841 | 1.500 | 0.530 | 0.516–0.544 | 1.754 |
| multichannel (DR_Multichannel_7.1_single_source.aaf) | parse | 0.776 | 0.804 | 1.020 | 0.958–1.086 | 1.578 |
| embedded (PT_PCM_Internal.aaf) | parse | 1.136 | 0.797 | 0.671 | 0.618–0.727 | 1.552 |
| embedded (PT_PCM_Internal.aaf) | extract | 0.089 | 0.063 | 0.672 | 0.596–0.756 | 1.354 |

The large fixture is the largest AAF in the pinned upstream suite (713,728 bytes); additional Downloads sessions were tested for correctness, including an 8,050,176-byte AAF. Larger designated performance workloads are still needed before final acceptance.

## Environment

```text
2026-10-06T20:14:47.715770+00:00
ProductName:		macOS
ProductVersion:		27.0.1
BuildVersion:		26A434
Apple M5
RAM: 17179869184 bytes
rustc 1.99.0 (b940084d7 2026-09-28)
Apple clang version 21.0.0 (clang-2100.3.34.2)
Target: arm64-apple-darwin27.0.0
Thread model: posix
InstalledDir: /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin
```

## Reproduce

```sh
cargo build --release -p libaaf-acceptance
cc -O3 -DNDEBUG -I tooling/reference/include tooling/benchmark.c tooling/reference-build/lib/aaf -o tooling/reference-build/benchmark
python3 tooling/benchmarks.py --pairs 21 --iterations 100
```

On Linux, add `-lm` when linking the C harness. This benchmark script intentionally requires the Mac acceptance machine. Shared CI performance is informational. Local raw reports are written under ignored `tooling/results`; the initial sanitized report is checked in as `docs/benchmark-baseline.json`.

## Initial heap profile

A single held parse of `PT_Fades.aaf`, sampled using macOS `heap -s` and `vmmap -summary`, measured 2,387,040 allocated bytes in Rust and 1,120,800 in C before compaction. Physical footprint was 4,496 KiB versus 2,816 KiB. These held-process measurements diagnose allocations; they do not replace the paired peak-RSS gate above. Rust's 704 KiB file allocation and spare capacity in node/object arrays were visible in the profile.

Both harnesses support `profile <file> <seconds> <unused-output-directory>` and print the process ID. Run `heap -s <pid>` and `vmmap -summary <pid>` while that document is held. Directory-name indexing and node/object capacities have since been compacted; final acceptance remains pending.
