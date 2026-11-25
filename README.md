# Adsertio

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Adsertio checks an AI agent's claims against proof that already exists on disk.

An agent says "the retry change passes" or "no test misses that branch". Usually the proof is already sitting in files that other tools wrote:

- [Trammel](https://github.com/IronAdamant/Trammel) records plan steps and whether each one was verified.
- [Chisel](https://github.com/IronAdamant/Chisel) records test results and which tests touch which files.
- [Probatio](https://github.com/IronAdamant/Probatio) records mutation runs: small deliberate bugs, and whether a test caught each one.

Adsertio ties a one-line claim to that proof. Each time you look at the claim, it reads those files again and gives one of three answers:

- **verified**: there is evidence, and all of it passes
- **refuted**: some evidence fails
- **unverified**: anything else, including "I cannot read this file"

The answer is worked out fresh on every view and is never stored. A file Adsertio does not understand never counts as proof.

What it deliberately does not do:

- run Trammel, Chisel, or Probatio, or write into their folders
- copy their data (a claim holds pointers, not copies)
- call a model
- plan work, run tests, or change code

Adsertio is one small Rust program with no dependencies beyond the standard library. It works as a command line tool (`adsertio`) and as an MCP server for AI agents (`adsertio-mcp`, or `adsertio mcp`).

## Install

Download the archive for your platform from the [latest release](https://github.com/IronAdamant/Adsertio/releases/latest):

| Platform | Archive |
|---|---|
| Linux x64 | `adsertio-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `adsertio-<version>-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `adsertio-<version>-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `adsertio-<version>-x86_64-pc-windows-msvc.zip` |

Each archive holds `adsertio` and `adsertio-mcp`. Check the download against `SHA256SUMS` before you run it. Adsertio is not on crates.io or npm.

Or build it yourself (Rust 1.83 or newer):

```sh
git clone https://github.com/IronAdamant/Adsertio
cd Adsertio
cargo build --release --locked
```

The binaries land in `target/release/`.

## A first claim

```sh
cd your-project
adsertio init                          # creates .adsertio/ here

adsertio claim --summary "Retry step passed" \
  --evidence 'trammel:.trammel/store#plan-1/step-1'

adsertio attach --id retry-step-passed \
  --evidence 'chisel:.chisel/store#test:tests/a.rs:t1'

adsertio view                          # the verdict for every claim
```

The claim id comes from the summary (`retry-step-passed`), or you can pick one with `--id`. Evidence paths are relative to the project, which is the folder holding `.adsertio/`.

`adsertio view --compact` gives a short answer (trimmed here):

```json
{
  "claims": [
    {
      "id": "retry-step-passed",
      "summary": "Retry step passed",
      "verdict": "verified",
      "evidence": [
        { "pointer": "trammel:.trammel/store#plan-1/step-1", "state": "pass" }
      ]
    }
  ],
  "counts": { "verified": 1, "refuted": 0, "unverified": 0 }
}
```

Leave out `--compact` and each pointer also carries a `detail` with the reason, such as `plan 1 step 1 verification.passed is true`.

Other commands:

- `adsertio ls` lists claims (id, summary, number of pointers) without reading any evidence.
- `adsertio check` reads every claim and fails if any is damaged.
- `adsertio guide` prints a short reminder of pointer shapes and the verdict rule.

Each flag is given once, except `--evidence`, `--tags` and `--states`, which repeat. An unknown or repeated flag is a usage error (exit code 2). A value that starts with `--` goes after `=` (`--summary=--fast`) or after a lone `--` (`--summary -- --fast`).

A claim is a small markdown file in `.adsertio/claims/`:

```markdown
---
summary: The retry change passes its plan step
author: main
created: 2026-11-25T02:05:00Z
evidence: [trammel:.trammel/store#plan-1/step-1]
---
```

There is no verdict in the file. That is on purpose: the verdict is only ever computed.

## Pointers

A pointer is `tool:path#selector`. The full rules are in [SPEC.md](SPEC.md). In short:

| Pointer | Passes when |
|---|---|
| `trammel:.trammel/store#plan-<id>/step-<id>` | that step was verified and passed |
| `trammel:.trammel/store#plan-<id>` | the plan has at least one step, and every step passed |
| `trammel:.trammel/store#constraint-<id>` | the constraint exists and is not marked inactive |
| `trammel:.trammel/store#failure-<id>` | the failure record exists |
| `chisel:.chisel/store#test:<test_id>` | the latest result for that test passed |
| `chisel:.chisel/store#file:<path>` | at least one test touches the file, and every such test last passed |
| `probatio:<out>` or `probatio:<out>#survivors` | the run finished, at least one mutant was caught, and none got through |
| `probatio:<out>#mutant:<id>` | that one mutant was caught (`killed`) |
| `probatio:<out>#ledger` | never: an unchecked ledger is not proof |

For Probatio, "finished" means no result is `flaky`, `error`, or `no coverage`, and every mutant in the mutant lists the run's `run.json` names has a result. If Adsertio cannot find or read those lists (no `run.json`, a list outside the project, a deleted patches folder), the run is `inconclusive`, not a pass.

A `probatio mutate diff` run is always `inconclusive`. Probatio does not write down the full list of mutants a diff plans, so a call capped by `--max-mutants` looks the same on disk as a whole one. Point at one mutant (`#mutant:<id>`), or score with `probatio mutate generate` and `probatio mutate run` to get a whole run Adsertio can check.

### When a pass does not count

A pass recorded **before** the claim was written does not verify it, and neither does a pass recorded against a commit that is not the current `HEAD`. Either one shows as `stale`, and the claim stays unverified. A failure always refutes, however old it is. So the normal order is: write the claim, produce the evidence, then view.

One exception: Probatio records no time or commit, so a Probatio pass is never stale. An old run can still verify a new claim, and the detail says `no timestamp`.

The only outside program Adsertio runs is `git rev-parse HEAD`, for the commit check.

### Pointer states

| State | Meaning |
|---|---|
| `pass` | The evidence says the claim holds. |
| `fail` | The evidence says it does not. |
| `missing` | The file or record is not there yet, or a step has not been verified. |
| `stale` | It passed, but before the claim, or on a different commit. |
| `inconclusive` | Read fine, but neither a pass nor a fail (for example, a mutation run with no catch and no survivor). |
| `unreadable` | Adsertio could not safely read or understand it. |
| `no_adapter` | Recognised, but deliberately not read (Chisel worktree overlays, and a pointer at one Chisel shard). |

Only `pass` counts towards verified, and only `fail` refutes.

## Long lists

With no limit, `view` and `ls` return every claim. For a big store, pass `--limit N` (1 to 500) and follow the `hint` in the reply. It gives the next call, using the `next_after` cursor. Claims come oldest first, and the cursor means a verdict changing on an earlier claim never makes you skip a later one.

`view --states verified --states refuted` keeps only those verdicts. On a page, `counts` covers the claims that page looked at, and `counts_complete` says whether that was all of them.

## Using it from an agent (MCP)

For Claude Code (`.mcp.json`) or Cursor (`.cursor/mcp.json`):

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
| `adsertio_claim` | Record a claim. Never overwrites one. |
| `adsertio_attach` | Add pointers to an existing claim. A bad or duplicate pointer writes nothing. |
| `adsertio_view` | The verdict and the state of every pointer. |
| `adsertio_ls` | Id, summary, and pointer count. No verdict. |
| `adsertio_check` | Fail on any damaged claim. Also clears a lock or temp file left by a writer that is gone. |
| `adsertio_guide` | Pointer shapes, the verdict rule, paging, and what to do after the conversation is compacted. |

Each tool is also a CLI command without the `adsertio_` prefix.

The server has one tool group, `core`, which is always on. You can still name it in the `agent-tools` block of a project's AGENTS.md; see [AGENTS.snippet.md](AGENTS.snippet.md). A mistake in that line falls back to `core` and says why.

````markdown
```agent-tools
adsertio: tools core
```
````

## Built to fail loudly

When something is wrong, Adsertio says so instead of guessing. Each of these is covered by tests in `tests/`:

- **Damaged claims.** A damaged claim stops the whole view and list with an error naming the file. It is never skipped, and the store is never reset.
- **Files it cannot trust are not proof.** A path that leaves the project, a Trammel or Chisel store over 1 GiB (Chisel's own limit), any other evidence file over 64 MiB, a file that is not UTF-8, a damaged Trammel or Chisel store, or a broken Probatio result is `unreadable`, never an empty pass. An old SQLite store gets a hint to run `trammel migrate` or `chisel migrate`.
- **Probatio runs must be finished.** An outcome Probatio does not write is `unreadable`. A `flaky`, `error` or `no coverage` result, a run with results missing, or a run whose mutant list cannot be checked, is `inconclusive`.
- **Chisel results come from the main store.** Chisel records test results only in `.chisel/store`. A pointer at it also reads the test edges in every shard, however the path is written (`./`, `//`, or through a symlink), but a shard's own result rows are never used. A pointer into `.chisel/overlays/` is not read, whatever its letter case or however a symlink reaches it.
- **Safe writes.** Writes land whole through a temp file. A new claim never overwrites another, and attach replaces only its own claim, under a lock. If an attach is killed, exactly one later writer takes over its lock, so concurrent attaches never lose a pointer.
- **Hands off other tools' files.** Adsertio takes a shared lock when Trammel or Chisel has a lock file, and never creates their locks or deletes their temp files or journals.
- **A strict server.** Bad JSON, oversized messages and batches get a JSON-RPC error and the server keeps running. Unknown methods and misspelt arguments are errors. A tool that panics is contained.

## Performance

1.2.0 indexes each Trammel or Chisel file once per call, and judges only the claims on the requested page. On a large test store (20,000 claims pointing into a 100,000-step Trammel file and a 200,000-result Chisel file):

| Case | 1.1.1 | 1.2.0 |
|---|---:|---:|
| `view --limit 500` | 249 s, 238 MiB | 0.48 s, 72 MiB |
| `check`, 1,000 claims | 199 s, 153 MiB | 0.34 s, 63 MiB |
| `view`, 32 claims, one 63 MiB Trammel file | 0.35 s, 191 MiB | 0.13 s, 129 MiB |

A full `view` of all 20,000 claims took 0.50 s. Large evidence files are still read whole (a read holds about the file's size in memory, plus its index), and `--limit 1` still reads every evidence file. The machine, method and full tables are in [docs/perf/1.2.0.md](docs/perf/1.2.0.md).

## Build and test

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

`Cargo.toml` has no dependencies; `tests/zero_deps.rs` checks that, and `Cargo.lock` holds one package.

Pushing a tag named `v*` (for example `v1.2.2`) builds the release archives and `SHA256SUMS` and attaches them to a GitHub release. The v1.0.0 release used shorter archive names, such as `adsertio-linux-x64.tar.gz`.

## Licence

MIT. See [LICENSE](LICENSE).
