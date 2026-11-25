# Adsertio

## What it is

Adsertio keeps one claim in one file: a single line, plus pointers at proof that already exists on disk. When you ask, it reads those files and says whether the claim holds.

## Why it exists

An agent says "the retry change passes" or "no test misses that branch". The proof is already there, scattered: a Trammel plan step, a Chisel test result, a Probatio mutant that no test caught. Nothing ties the claim to that proof and says, honestly, whether it holds.

A claim is one line plus pointers at files those tools already wrote. On every view, Adsertio reads those files and reports **verified**, **refuted**, or **unverified**, with a reason for each pointer. Verified needs evidence, and all of it passing. Any failing evidence refutes. Everything else, including "I cannot read this", is unverified. A file Adsertio does not understand is not proof. The verdict is worked out on every view and is never stored.

It deliberately does not:

- run Trammel, Chisel, or Probatio, or write into `.trammel/`, `.chisel/`, or a Probatio output folder
- copy their data into its own store (pointers only)
- call a model
- plan work, run tests, or mutate code
- treat a file it does not understand as a pass

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Standard library only, no Cargo crates. One binary: the `adsertio` CLI and the `adsertio-mcp` stdio server (`adsertio mcp` is the same program).

## Install

A tag named `v*` (for example `v1.1.0`) builds the release binaries and attaches them to a GitHub release, with `SHA256SUMS`. The archive name is `adsertio-<version>-<target-triple>`:

- Linux x64 (`adsertio-<version>-x86_64-unknown-linux-gnu.tar.gz`)
- macOS x64 (`adsertio-<version>-x86_64-apple-darwin.tar.gz`)
- macOS arm64 (`adsertio-<version>-aarch64-apple-darwin.tar.gz`)
- Windows x64 (`adsertio-<version>-x86_64-pc-windows-msvc.zip`, entries use forward slashes)

The v1.0.0 release used shorter names: `adsertio-linux-x64.tar.gz`, `adsertio-macos-x64.tar.gz`, `adsertio-macos-arm64.tar.gz`, and `adsertio-windows-x64.zip`.

Each archive contains `adsertio` and `adsertio-mcp` (the same program). Check the file against `SHA256SUMS` before you run it. Adsertio is not on crates.io, and there is no npm package.

Or build it (Rust 1.83 or newer):

```sh
git clone https://github.com/IronAdamant/Adsertio
cd Adsertio
cargo build --release --locked
```

That writes `target/release/adsertio` and `target/release/adsertio-mcp`.

## Usage

```sh
cd your-project
adsertio init
adsertio claim --summary "Retry step passed" \
  --evidence 'trammel:.trammel/store#plan-1/step-1'
adsertio attach --id retry-step-passed \
  --evidence 'chisel:.chisel/store#test:tests/a.rs:t1'
adsertio view
adsertio ls
adsertio check
```

`init` creates `.adsertio/` in the current folder. The project is the folder that holds it. Evidence paths are resolved inside that folder.

A claim file looks like this:

```markdown
---
summary: The retry change passes its plan step
author: main
created: 2026-11-25T02:05:00Z
evidence: [trammel:.trammel/store#plan-1/step-1]
---
```

There is no `verdict` field. `adsertio view` computes it. `adsertio ls` lists id, summary, and pointer count, and does not read the evidence. With no `limit` or `offset`, both return every matching claim. Pass `limit` (1 to 500) and `offset` to page. When `truncated` is true, `hint` gives the next call and `next_offset` is the offset to pass. `count` and `counts` still cover the whole store. `adsertio view --states` keeps one or more of verified, refuted, and unverified. `--compact` drops evidence detail. `adsertio guide` (MCP name `adsertio_guide`) is the short reminder of pointer shapes and what to call after a conversation is compacted.

Pointers are `tool:path[#selector]`. The tool is `trammel`, `chisel`, or `probatio`. The path is inside the project. Selectors are in [SPEC.md](SPEC.md). In short:

| Pointer | Passes when |
|---|---|
| `trammel:.trammel/store#plan-<id>/step-<id>` | that step's `verification.passed` is true |
| `trammel:.trammel/store#plan-<id>` | every step of the plan passed, and there is at least one |
| `chisel:.chisel/store#test:<test_id>` | the latest result for that test passed |
| `chisel:.chisel/store#file:<path>` | every test that touches that file last passed, and at least one does |
| `probatio:<out>#survivors` (or no selector) | at least one mutant was killed and none survived |
| `probatio:<out>#mutant:<id>` | that mutant's outcome is `killed` |

A pass recorded before the claim was written, or against a commit that is not `HEAD`, does not verify the claim. A failure still refutes it. `git rev-parse HEAD` is the only external program, and only for that check.

## MCP tools

Claude Code (`.mcp.json`) or Cursor (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "adsertio": {
      "command": "adsertio-mcp",
      "args": [],
      "env": { "ADSERTIO_DIR": "/absolute/path/to/project/.adsertio" }
    }
  }
}
```

`ADSERTIO_DIR` is only needed when the host does not start the server inside the project.

| Tool | Does |
|---|---|
| `adsertio_claim` | Record a claim. Never overwrites. |
| `adsertio_attach` | Add pointers to an existing claim. A bad or duplicate pointer writes nothing. |
| `adsertio_view` | Verdict and the state of every pointer. One page. |
| `adsertio_ls` | Id, summary, and pointer count. No verdict. One page. |
| `adsertio_guide` | Pointer shapes, the verdict rule, paging, and what to call after compaction. |

The same names without the `adsertio_` prefix are CLI commands, plus `init`, `check`, and `mcp`.

Per pointer the state is `pass`, `fail`, `missing`, `unreadable`, `no_adapter`, `inconclusive`, or `stale`. Per claim:

- **verified**: there is evidence and every pointer passed
- **refuted**: any pointer failed
- **unverified**: everything else

No claim is verified without a passing adapter result.

## Choosing tool groups

Add a line to the `agent-tools` block in the project's AGENTS.md. The server rereads it on every request. There is one group, `core`, and it cannot be switched off. A mistake in the line falls back to the core tools and says why. See [AGENTS.snippet.md](AGENTS.snippet.md).

````markdown
```agent-tools
adsertio: tools core
```
````

## Built to fail loudly

Covered by tests in `tests/`:

- Deeply nested JSON, invalid UTF-8, oversized lines, and batches get a JSON-RPC error. The server keeps running.
- Unknown methods are errors. A failed tool call sets `isError`. A panicking tool is contained.
- Unknown or misspelled arguments are errors.
- A damaged claim fails the whole view and the whole list. It is never skipped, and the store is never reset.
- Evidence paths that leave the project, folders (except a Probatio output folder), files over 64 MiB, and files that are not valid UTF-8 are `unreadable`.
- A file that starts with the SQLite header is `unreadable`, with a hint to run `trammel migrate` or `chisel migrate`. There is no SQLite reader. The live stores are JSON.
- A damaged Trammel or Chisel snapshot is `unreadable`, not an empty pass. A Trammel journal line that is not valid JSON fails the read. A Chisel compact table that cannot be expanded fails the read.
- A broken Probatio `results/*.json` makes that run `unreadable`. It is not dropped.
- Writes to `.adsertio/` land whole through a temp file. A new claim never overwrites. Attach replaces only that one claim file.
- The reader takes a shared lock when `.trammel/lock` or `.chisel/chisel.lock` already exists, and it does not create those files or delete `store.tmp` or a journal.

## Build and test

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Rust 1.83. `Cargo.toml` has no dependencies. `tests/zero_deps.rs` checks that, and `Cargo.lock` holds one package.

## Licence

MIT. See [LICENSE](LICENSE).
