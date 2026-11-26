# Adsertio

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Adsertio checks an AI agent's claims against proof that already exists on disk.

An agent says "the retry change passes" or "no test misses that branch". Usually the proof is already sitting in files that other tools wrote:

- [Trammel](https://github.com/IronAdamant/Trammel) records plan steps and whether each one was verified.
- [Chisel](https://github.com/IronAdamant/Chisel) records test results and which tests touch which files.
- [Probatio](https://github.com/IronAdamant/Probatio) records mutation runs: small deliberate bugs, and whether a test caught each one.
- [Relatum](https://github.com/IronAdamant/Relatum) keeps notes in `.swarmnotes/`, including the project's rules (constraints, conventions, decisions), and marks a rule superseded when a newer one replaces it.

Adsertio ties a one-line claim to that proof. Each time you look at the claim, it reads those files again and gives one of three answers:

- **verified**: there is evidence, and all of it passes
- **refuted**: some evidence fails
- **unverified**: anything else, including "I cannot read this file"

The answer is worked out fresh on every view and is never stored. A file Adsertio does not understand never counts as proof.

What it deliberately does not do:

- run Trammel, Chisel, Probatio, or Relatum, or write into their folders
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

To record several claims, pass them all at once. Each item has the same fields as one claim (`summary`, and optionally `evidence`, `id`, `author`, `body`, `tags`), and a call takes up to 50:

```sh
adsertio claim --claims '[{"summary":"Retry step passed","evidence":["trammel:.trammel/store#plan-1/step-1"]},{"summary":"Parser test passes","id":"parser","evidence":["chisel:.chisel/store#test:tests/a.rs:t1"]}]'
```

Every item is checked before anything is written. If one is bad (a bad pointer, an id that already exists, two items with the same id), nothing is written and the error names the item. Quoting JSON differs between cmd, PowerShell and Git Bash, so on Windows put the array in a file and pass `--claims-file claims.json`. `adsertio attach --items '[{"id":"a","evidence":[...]}]'` (or `--items-file`) adds pointers to several claims the same way.

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
- `adsertio doctor` says what is wrong with the set-up, and how to fix it (below).
- `adsertio hook --host claude|cursor|generic` prints a hook to paste into the host's settings (below).

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
| `relatum:.swarmnotes#<note-id>` | the Relatum note exists, its frontmatter is sound, and it is active (a superseded note fails, a draft is `inconclusive`) |

For Probatio, "finished" means no result is `flaky`, `error`, or `no coverage`, and every mutant in the mutant lists the run's `run.json` names has a result. If Adsertio cannot find or read those lists (no `run.json`, a list outside the project, a deleted patches folder), the run is `inconclusive`, not a pass.

A Relatum note id is its path under the store without `.md`, such as `net/retry-decision`. When a rule is superseded, a claim that points at it is refuted, and the detail names the note that replaced it: point the claim at that note instead. Adsertio does not follow the supersede chain, so a pointer always judges the note it names. A note Relatum could not read (no frontmatter, no `kind` or `summary`, an unknown `status`, or `superseded` with no `superseded_by`) is `unreadable`.

`adsertio claim` and `adsertio attach` refuse a pointer whose selector can never be valid (a Trammel or Chisel pointer with no selector, an unknown selector, an empty or bad id, or a Relatum path that names the note's `.md` file instead of the store folder), and nothing is written. The one-line error names the pointer and the expected shape, and suggests a fix when the text makes it plain: `relatum:_shared/no-deps` gets `did you mean relatum:.swarmnotes#_shared/no-deps?`. Whether the file, row or note is there is still judged when the claim is viewed.

A `probatio mutate diff` run is always `inconclusive`. Probatio does not write down the full list of mutants a diff plans, so a call capped by `--max-mutants` looks the same on disk as a whole one. Point at one mutant (`#mutant:<id>`), or score with `probatio mutate generate` and `probatio mutate run` to get a whole run Adsertio can check.

### When a pass does not count

A pass recorded **before** the claim was written does not verify it, and neither does a pass recorded against a commit that is not the current `HEAD`. Either one shows as `stale`, and the claim stays unverified. A failure always refutes, however old it is. So the normal order is: write the claim, produce the evidence, then view.

One exception: Probatio records no time or commit, so a Probatio pass is never stale. An old run can still verify a new claim, and the detail says `no timestamp`.

A Relatum rule is never stale either. Like a Trammel `constraint-<id>`, a rule is a state, not an event: it is normal for the rule to be written before the claim, and what matters is whether it is still active when you look.

Testing uncommitted work and then committing it is the usual order, and it moves `HEAD`. Chisel 0.25.0 and later also record `tree`, the git tree of the working tree the tests ran against (uncommitted and untracked files included). A Chisel pass whose commit is not `HEAD` still counts when that tree is `HEAD`'s tree, or differs from it only under `.adsertio`, `.chisel`, `.trammel` or `.probatio` (claims and evidence, not code): the code that was tested is exactly the committed code, and the detail says so. Any other change committed after the run makes it `stale`. An older Chisel records no tree, and Trammel records none, so for those the commit must be `HEAD`.

The only outside program Adsertio runs is git: `git rev-parse HEAD HEAD^{tree}` once per view, and `git diff-tree` once per recorded tree that is not `HEAD`'s tree.

When a Chisel or Trammel pass is stale because its commit is not `HEAD`, the detail ends with the fix: rerun the tests at `HEAD`, for example `chisel run -- <test command>`. When Chisel recorded no tree, it adds that Chisel 0.25.0 or later records one, so committing exactly the tested work needs no rerun.

### Pointer states

| State | Meaning |
|---|---|
| `pass` | The evidence says the claim holds. |
| `fail` | The evidence says it does not. |
| `missing` | The file or record is not there yet, or a step has not been verified. |
| `stale` | It passed, but before the claim, or on a different commit (and, for Chisel, on code that is not the committed code). |
| `inconclusive` | Read fine, but neither a pass nor a fail (for example, a mutation run with no catch and no survivor, or a Relatum draft). |
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
| `adsertio_claim` | Record claims. Pass `claims`, a list of up to 50, to record them all in one call. Never overwrites one. |
| `adsertio_attach` | Add pointers to existing claims. Pass `items`, a list of `{id, evidence}`, for several claims in one call. A bad or duplicate pointer writes nothing. |
| `adsertio_view` | The verdict and the state of every pointer. |
| `adsertio_ls` | Id, summary, and pointer count. No verdict. |
| `adsertio_doctor` | What is wrong with the store, damaged claims, git, or the evidence files, each with a fix. `fix: true` clears temp files over an hour old and lock files left by Adsertio 1.3. |
| `adsertio_guide` | Pointer shapes, the verdict rule, paging, and what to do after the conversation is compacted. |

Each tool is also a CLI command without the `adsertio_` prefix. `adsertio check`, which fails on any damaged claim, is a CLI command only (for scripts and CI); over MCP, `adsertio_doctor` reports the same problems.

Each MCP call is one agent turn, and every turn reads the whole conversation again, so an agent should record all its claims in one `adsertio_claim` call and attach in one `adsertio_attach` call. The tool descriptions say so.

The tool definitions are kept small for the same reason: the host sends them on every turn. The `tools` array of `tools/list` stays at or under 4,000 bytes and the server `instructions` under 500 bytes (`tests/tool_budget.rs` checks both). Descriptions say what a tool does, when to call it, and the one thing to get right; the rest is in `adsertio_guide`, which an agent calls when it needs it.

## After compaction: the session hook

When a conversation is compacted, an agent can remember a verdict that is no longer true. `adsertio hook` prints a hook that runs, at session start and after compaction:

```sh
adsertio view --compact --states unverified --states refuted --limit 50 --if-store
```

so the agent sees the claims that are not verified and reads them again instead of repeating a verdict from memory. `--if-store` makes it print nothing (and exit 0) in a project with no `.adsertio` folder, so the hook can go in your user settings and stay quiet where Adsertio is not used.

- `--host claude` prints JSON for `.claude/settings.json`: a `SessionStart` hook whose matcher is `startup|resume|clear|compact`, and a `SubagentStart` hook that gives a sub-agent the same list. It uses the exec form (`"command": "adsertio"` with an `args` list), which starts the program with no shell, so it works the same on Windows, macOS and Linux.
- `--host cursor` prints JSON for `.cursor/hooks.json`. Its command adds `--wrap cursor`, which prints the view inside the `additional_context` JSON Cursor reads. Cursor's `preCompact` cannot put text back, so after compaction run the view again yourself.
- `--host generic` (the default) prints only the command.

`adsertio` must be on `PATH`. The command has no shell syntax (no `&&`, quotes or `~`). Adsertio prints the snippet; it does not edit your settings.

## When something is wrong: doctor

`adsertio doctor` (or the `adsertio_doctor` tool) runs a few cheap checks and prints JSON: `ok`, and `checks`, each `{name, ok, detail, fix}`. Every failing check has a `fix` sentence. It checks:

- the version and platform
- where the store is, and `ADSERTIO_DIR`
- that the claims folder takes a new file (it writes and removes a probe the way a new claim is written)
- the claim count, any damaged claim, and any pass whose timestamp or commit is malformed
- temp files over an hour old, and attach lock files left in `claims` by Adsertio 1.3
- whether the store is on a network file system, where file locks may not hold between machines (a warning; the check still passes)
- `git` on `PATH`, and what `git rev-parse HEAD` says
- `.trammel/store`, `.chisel/store` and `.probatio`: there or not, size against the read limits, and whether Adsertio can read them
- `.swarmnotes`: there or not, and whether Adsertio can read every note in it (with a count of active, draft and superseded notes)
- that `adsertio-mcp` sits next to `adsertio`
- on Windows, how close the store path is to the 260-character limit

It exits 0 when every check passes and 1 when one fails. Doctor only reads. `doctor --fix` also removes those temp and 1.3 lock files, which `check` already does, and lists them in `fixed`. It never repairs a claim and never touches another tool's files.

The server has one tool group, `core`, which is always on. You can still name it in the `agent-tools` block of a project's AGENTS.md; see [AGENTS.snippet.md](AGENTS.snippet.md). A mistake in that line falls back to `core` and says why.

````markdown
```agent-tools
adsertio: tools core
```
````

## Upgrading

Claim files keep the same format from 1.2 on, so a store needs no migration.
What changes for an agent or a script, newest first (all detail is in
[CHANGELOG.md](CHANGELOG.md)):

- **To 1.5:** `relatum:<store>#<note-id>` is a fourth evidence tool. `claim`
  and `attach` now refuse a pointer whose shape can never be valid (no
  selector, a malformed one, a Relatum path that names a note file), where
  1.4 stored it and only showed it as `unreadable` at view. The error names
  the expected shape and, where it can, the corrected pointer. Claims already
  stored view exactly as before. `doctor` gains a `relatum` check.
- **To 1.4:** attach locks are operating-system file locks. Stop every 1.3
  writer before the first 1.4 write; `doctor --fix` and `check` remove the
  lock files 1.3 left. A Chisel 0.25+ pass stays fresh after you commit
  exactly the tested tree.
- **To 1.3:** `adsertio_claim` and `adsertio_attach` take lists (`claims`,
  `items`), so their schemas no longer mark `summary`, `id` or `evidence` as
  required; a batch replies `{ids, count}` or `{claims: [...]}`.
  `adsertio_doctor` arrived, and in 1.3.1 `adsertio_check` folded into it,
  so `tools/list` has six tools.

## Built to fail loudly

When something is wrong, Adsertio says so instead of guessing. Each of these is covered by tests in `tests/`:

- **Damaged claims.** A damaged claim stops the whole view and list with an error naming the file. It is never skipped, and the store is never reset.
- **Files it cannot trust are not proof.** A path that leaves the project, a Trammel or Chisel store over 1 GiB (Chisel's own limit), any other evidence file over 64 MiB, a file that is not UTF-8, a damaged Trammel or Chisel store, a broken Probatio result, or a Relatum note with damaged frontmatter is `unreadable`, never an empty pass. An old SQLite store gets a hint to run `trammel migrate` or `chisel migrate`.
- **Probatio runs must be finished.** An outcome Probatio does not write is `unreadable`. A `flaky`, `error` or `no coverage` result, a run with results missing, or a run whose mutant list cannot be checked, is `inconclusive`.
- **Chisel results come from the main store.** Chisel records test results only in `.chisel/store`. A pointer at it also reads the test edges in every shard, however the path is written (`./`, `//`, or through a symlink), but a shard's own result rows are never used. A pointer into `.chisel/overlays/` is not read, whatever its letter case or however a symlink reaches it.
- **Safe writes.** Writes land whole through a temp file. A new claim never overwrites another, and attach replaces only its own claim, under an operating-system lock on `.adsertio/locks/<id>.lock` (`flock` on Unix, `LockFileEx` on Windows). Attaches to one claim take turns, so none loses a pointer; attaches to different claims run side by side. If an attach is killed, the kernel releases its lock at once: there is no stale lock to wait out or clear.
- **Hands off other tools' files.** Adsertio takes a shared lock when Trammel or Chisel has a lock file, and never creates their locks or deletes their temp files or journals. Relatum has no lock file (it publishes each note whole), so a note is read plainly, and nothing is ever written into `.swarmnotes`.
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

Pushing a tag named `v*` (for example `v1.3.0`) builds the release archives and `SHA256SUMS` and attaches them to a GitHub release. The v1.0.0 release used shorter archive names, such as `adsertio-linux-x64.tar.gz`.

## Licence

MIT. See [LICENSE](LICENSE).
