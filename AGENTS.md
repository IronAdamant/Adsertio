# Adsertio

One verified-or-unverified view per claim, read from the evidence Trammel, Chisel
and Probatio leave on disk. One std-only Rust crate: the `adsertio` CLI and the
`adsertio-mcp` stdio server (`adsertio mcp` is the same program). Read `SPEC.md`
for the format and `README.md` for what is built.

## Rules

- `[dependencies]`, `[dev-dependencies]`, and `[build-dependencies]` stay empty;
  `tests/zero_deps.rs` checks it, and `Cargo.lock` holds one package. Do not add
  a crate.
- Never call a model, and never run Trammel, Chisel or Probatio. Read their files.
- Read the other tools' files read-only. Never write into their stores. Do not
  create their lock files, and do not delete `store.tmp` or a journal.
- Do not duplicate what they hold. A claim carries pointers, not copies.
- Anything Adsertio cannot interpret is not proof. An adapter that does not
  understand a file returns `unreadable`, never `pass`.
- The verdict is computed on every view. Do not store it.
- No TODO or FIXME markers in `src/`.
- Docs use Australian spelling and plain ASCII: no em-dashes.
- A damaged claim is an error. Never skip it, never reset the store.
- Evidence paths go through `fsx::project_file`; ids through `fsx::join_id`.
- Writes go through `fsx::write_atomic`. New claims use `overwrite = false`.
  Attach overwrites only that claim file.
- Unknown arguments are errors. New tools go in the `tools::TOOLS` table.

## Commands

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

## Layout

- `src/json.rs` bounded JSON. `src/json_tests.rs` parser tests.
- `src/fsx.rs` atomic writes, path and slug guards. `src/proc.rs` live pids.
- `src/lock.rs` shared locks. `src/read.rs` snapshots, the journal, compact tables.
- `src/trammel.rs`, `src/chisel.rs`, `src/probatio.rs` the adapters.
- `src/trammel_idx.rs`, `src/chisel_idx.rs` one index per snapshot per call.
- `src/paging.rs` view pages and the `after` cursor.
- `src/cache.rs` reuses one indexed snapshot, and one `git rev-parse HEAD`, for every claim in a single view or check.
- `src/fresh.rs` timestamp and commit checks. `src/verdict.rs` states and `combine`.
- `src/tool.rs` tool table types and argument validation. `src/tools.rs` the tools.
- `src/mcp.rs` the server. `src/groups.rs` the `agent-tools` block.
- `src/claim.rs` claims and pointers. `src/store.rs` store lookup. `src/cli.rs`
  the command line.
- `.github/workflows/release.yml` builds the GitHub release when a `v*` tag is pushed. No npm, no crates.io.
- `tests/hostile_input.rs`, `tests/claims_and_evidence.rs`,
  `tests/cli_and_groups.rs`, `tests/reader.rs`, `tests/trammel.rs`,
  `tests/chisel.rs`, `tests/probatio.rs`, `tests/fresh.rs`, `tests/attach_ls.rs`,
  `tests/load.rs`, `tests/review4.rs`:
  the failure modes the design avoids. Keep them passing.
