# Adsertio

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**One verified-or-unverified view per claim, read from the evidence other tools left on disk.**

An agent says "the retry change passes" or "no test misses that branch". The
proof already exists, scattered: a Trammel `verify_step` result, a Chisel test
run, a Probatio mutation survivor list. Adsertio ties each claim to pointers at
that evidence and reports one verdict: **verified**, **refuted** or **unverified**,
with the reason for every pointer.

It reads the other tools' files and does not duplicate them. It never calls a
model, and it never runs a tool for you. Standard library only, no Cargo crates,
a CLI and a stdio MCP server over the same files.

> **Status: v0.1.0, a scaffold.** Claims, pointers, the path and read guards, the
> verdict rules and the MCP server work and are tested. **The adapters that
> interpret Trammel, Chisel and Probatio output are not built**, so today no
> claim can reach "verified". That is deliberate: evidence Adsertio cannot read
> yet is reported as `no_adapter` and never counts as proof. See "What works
> today" and [SPEC.md](SPEC.md).

## How it works

```
.adsertio/
  claims/retry-passes.md        <- a claim and its evidence pointers
```

A claim is one line plus pointers, each `tool:path[#selector]`, where the tool is
`trammel`, `chisel` or `probatio` and the path is inside the project:

```markdown
---
summary: The retry change passes its plan step
author: main
created: 2026-11-25T02:05:00Z
evidence: [trammel:.trammel/store#plan-3/step-2, chisel:.chisel/store]
---
```

The verdict is never stored. `adsertio_view` works it out from the evidence each
time, so it cannot go stale:

- **verified**: there is evidence and all of it passes.
- **refuted**: any evidence fails.
- **unverified**: everything else, including no evidence, a missing file, an
  unreadable file, and evidence Adsertio has no adapter for. The view says which.

Per pointer the state is `pass`, `fail`, `missing`, `unreadable` or `no_adapter`.

## What it is not

| Tool | What it holds | Adsertio instead |
|---|---|---|
| Trammel | Plans, constraints, `verify_step` results, failure history | Reads them as evidence. Does not plan or verify. |
| Chisel | Test impact and results, coverage | Reads them as evidence. Does not run tests. |
| Probatio | Mutation runs and survivors | Reads them as evidence. Does not mutate. |
| Libellus | An agent's private notes and costs | A Libellus note can point at a claim as `adsertio:<id>`. |
| Relatum | Notes shared across a swarm | Claims are about evidence, not shared context. |

## What works today

| Tool | Group | Does |
|---|---|---|
| `adsertio_claim` | core | Record a claim with evidence pointers. Never overwrites. |
| `adsertio_view` | core | Each claim's verdict and the state of every pointer. |

The same as CLI commands without the `adsertio_` prefix:

```sh
cargo build --release
cd your-project
adsertio init
adsertio claim --summary "Retry change passes" --evidence trammel:.trammel/store
adsertio view
```

Evidence paths are resolved inside the project. Absolute paths, `..`, symlinks
that leave the project, folders, files over 64 MiB and files that are not valid
UTF-8 are `unreadable`, never read.

Not built yet: the three adapters, attaching evidence to an existing claim,
freshness (evidence older than the claim or the commit), release binaries and npm
packages. SPEC.md describes each.

## Built to fail loudly

Designed out from the start and covered by tests in `tests/`:

- Deeply nested JSON, invalid UTF-8, oversized lines and batches get a JSON-RPC
  error. The server keeps running.
- Unknown methods are errors. A failed tool call sets `isError`.
- Unknown or misspelled arguments are errors, never ignored.
- A damaged claim is an error naming the file. It is never skipped and the view
  is never empty because of it.
- Claim ids are slugs; evidence paths that leave the project are refused.
- Every write lands whole through a temp file cleaned up on failure; a claim is
  created with a hard link and never overwrites.
- A mistake in the `agent-tools` line falls back to the core tools and says why.

## Choosing tool groups

Add a line to the `agent-tools` block in the project's AGENTS.md; the server
rereads it on every request. Today there is one group, `core`, which cannot be
switched off, so the block is ready for later groups. See
[AGENTS.snippet.md](AGENTS.snippet.md).

## Build and test

```sh
cargo test --locked
cargo build --release --locked
```

Rust 1.83. `Cargo.toml` has no dependencies; `tests/zero_deps.rs` checks it.

## Licence

MIT. See [LICENSE](LICENSE).
