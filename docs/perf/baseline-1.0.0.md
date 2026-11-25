# Performance baseline, Adsertio 1.0.0

Measured before the changes in `docs/perf/1.1.0.md`. The harness is
`tests/perf.rs`. It does not run in CI.

```sh
cargo test --release --locked --test perf -- --ignored --nocapture
```

Times are one process per CLI or cold MCP call, and one live server for the
warm MCP samples. Median and p95 use nearest rank on the sorted samples.
`n` is the sample count.

## Machine

- OS: Linux 6.12.113+ x86_64
- CPU: Intel(R) Xeon(R) Processor (the model string from `/proc/cpuinfo`; no finer name was published)
- Memory: MemTotal 16387328 kB (about 16 GiB)
- Compiler: rustc 1.83.0 (90b35a623 2024-11-26)
- Build: `cargo test --release --locked`

This is a cloud VM. Absolute times will move on other hardware. The ratios in the follow-up note are the part to compare.

## Cold start and MCP

Empty store, except the MCP calls which use a store with no claims.

| Path | n | Median | p95 |
|---|---:|---:|---:|
| CLI `adsertio --version` | 31 | 727 us | 1087 us |
| MCP cold `adsertio_ls` (spawn, one call, exit) | 15 | 805 us | 926 us |
| MCP warm `adsertio_ls` (server already up) | 31 | 16 us | 75 us |

## Claims

Each claim is one small file, no evidence pointer. Directory size is `.adsertio/` only.

| Claims | Store bytes | Largest claim | `ls` median / p95 | `view` median / p95 | `view --id` median / p95 | `claim` median / p95 | `attach` median / p95 |
|---:|---:|---:|---|---|---|---|---|
| 10 | 690 | 69 | 640 / 880 us | 594 / 831 us | 692 / 994 us | 1449 / 2214 us | 1408 / 1808 us |
| 1,000 | 70,890 | 71 | 3.8 / 5.3 ms | 4.2 / 5.5 ms | 3.5 / 5.5 ms | 1.9 / 2.5 ms | 1.1 / 1.9 ms |
| 10,000 | 718,890 | 72 | 41.6 / 74.3 ms | 39.0 / 49.4 ms | 28.0 / 37.1 ms | 1.9 / 7.4 ms | 1.1 / 1.5 ms |

`view --id` still reads every claim file, because a damaged sibling fails the whole view. Samples: `ls` 11, `view` 9 (5 at 10,000), `view --id` 9, `claim` 7, `attach` 7.

## Evidence

One claim. The fixture is built in the temp project. `read_trammel` is in-process. The `view_*` rows are the CLI, so they include process start.

| Fixture | Bytes | View median / p95 | In-process read median / p95 |
|---|---:|---|---|
| Trammel store, 10 steps | 724 | 644 / 849 us | 12 / 30 us |
| Trammel store, 1,000 steps | 58,956 | 1.4 / 1.7 ms | 521 / 792 us |
| Trammel store, 10,000 steps | 607,957 | 7.6 / 10.0 ms | 5.2 / 10.0 ms |
| Chisel store, 10 results | 635 | 596 / 933 us | |
| Chisel store, 1,000 results | 57,963 | 1.2 / 1.3 ms | |
| Chisel store, 10,000 results | 588,962 | 5.6 / 5.7 ms | |
| Probatio `results/`, 10 files | 570 | 941 / 1047 us | |
| Probatio `results/`, 1,000 files | 58,890 | 7.0 / 10.3 ms | |
| Probatio `results/`, 10,000 files | 598,890 | 75.2 / 130.0 ms | |
| Ledger, 10 entries | 503 | 841 us / 9.0 ms | |
| Ledger, 1,000 entries | 45,953 | 1.4 / 1.9 ms | |
| Ledger, 10,000 entries | 468,953 | 8.4 / 11.4 ms | |

The ledger row at 10 entries has a p95 far above the median (9.0 ms against 841 us). That is one slow sample, not a trend at larger sizes.

## Repeated evidence

100 claims, each pointing at the same 1,000-step Trammel store (58,956 bytes).

| Path | n | Median | p95 |
|---|---:|---:|---:|
| `view` | 5 | 44.2 ms | 47.4 ms |

The store is parsed once per claim. That is the row the next change is aimed at.
