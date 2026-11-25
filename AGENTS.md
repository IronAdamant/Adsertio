# Adsertio

One verified-or-unverified view per claim, read from the evidence Trammel, Chisel
and Probatio leave on disk. One std-only Rust crate: the `adsertio` CLI and the
`adsertio-mcp` stdio server (`adsertio mcp` is the same program). Read `SPEC.md`
for the format and `README.md` for what is built.

## Rules

- `[dependencies]` and `[dev-dependencies]` stay empty; `tests/zero_deps.rs`
  checks it, and `Cargo.lock` holds one package. Do not add a crate.
- Never call a model, and never run Trammel, Chisel or Probatio. Read their files.
- Read the other tools' files read-only. Never write into their stores.
- Do not duplicate what they hold. A claim carries pointers, not copies.
- Anything Adsertio cannot interpret is not proof. An adapter that does not
  understand a file returns `unreadable`, never `pass`.
- The verdict is computed on every view. Do not store it.
- No TODO or FIXME markers in `src/`.
- Docs use Australian spelling and plain ASCII: no em-dashes.
- A damaged claim is an error. Never skip it, never reset the store.
- Evidence paths go through `fsx::project_file`; ids through `fsx::join_id`.
- Writes go through `fsx::write_atomic`. New claims use `overwrite = false`.
- Unknown arguments are errors. New tools go in the `tools::TOOLS` table.

## Commands

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

## Layout

- `src/json.rs` bounded JSON. `src/fsx.rs` atomic writes, path and slug guards.
- `src/tool.rs` tool table types and argument validation. `src/tools.rs` the tools.
- `src/mcp.rs` the server. `src/groups.rs` the `agent-tools` block.
- `src/claim.rs` claims and pointers. `src/verdict.rs` evidence states and the
  verdict; this is where the adapters go.
- `src/store.rs` store lookup. `src/cli.rs` the command line.
- `tests/hostile_input.rs`, `tests/claims_and_evidence.rs`,
  `tests/cli_and_groups.rs`: the failure modes the design avoids. Keep them passing.
