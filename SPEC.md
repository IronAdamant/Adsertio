# Adsertio format v1

Adsertio keeps claims and works out, on demand, whether the evidence behind
each one supports it. The claim files are the product. The CLI and MCP server
are conveniences over them. The evidence stays in the other tools' own files.

No claim is verified without a passing adapter result. A file the adapter
does not understand is `unreadable` or `no_adapter`, never `pass`.

## 1. Layout

```
.adsertio/
  claims/<id>.md        one claim per file; <id> is a slug
```

- The store is the nearest `.adsertio/` walking up from the working directory,
  or the folder named by `$ADSERTIO_DIR`. `adsertio init` creates it. No tool
  creates a store by accident.
- The **project** is the folder that holds `.adsertio/`. Evidence paths are
  resolved inside it.
- `<id>` is a slug: `[a-z0-9][a-z0-9-]*`, at most 64 bytes, one segment.
- A symlink in the claims folder, or any file that is not `<id>.md`, is an
  error for `check`. `view` and `ls` leave it out and warn on stderr.

## 2. Claims

```markdown
---
summary: The retry change passes its plan step
author: main
created: 2026-11-25T02:05:00Z
evidence: [trammel:.trammel/store#plan-1/step-1, chisel:.chisel/store#test:tests/a.rs:t1]
tags: [net]
---

Why this claim matters, in a few lines.
```

| Field | Required | Meaning |
|---|---|---|
| `summary` | yes | The claim, one line, up to 200 characters |
| `author` | yes | Agent name slug (default `agent`) |
| `created` | yes | UTC, `YYYY-MM-DDTHH:MM:SSZ` |
| `evidence` | no | Pointers (below) |
| `tags` | no | Slugs |

There is no `verdict` field. A key that is not listed, including `verdict`, is
an error: the verdict is computed, never stored. Duplicate keys, bad pointers, a
missing field, or an unclosed frontmatter are errors naming the file.

**Evidence pointer:** `<tool>:<path>[#selector]`.

- `tool` is `trammel`, `chisel`, or `probatio`.
- `path` is relative to the project, up to 300 characters in all, with no
  whitespace, commas, or brackets.
- `selector` chooses a row inside that file. The grammar is below.

Trammel and Chisel pointers with no selector are `unreadable` (a selector is
required) once the file itself can be read. A missing file is `missing`. A
Probatio pointer with no selector means the whole run.

## 3. Selectors

Ids in Trammel selectors are the row `id`, not `step_index`.

| Pointer | Selector | Pass when | Fail when | Otherwise |
|---|---|---|---|---|
| `trammel:.trammel/store` | `plan-<id>/step-<id>` | `verification.passed` is true | `verification` is present and `passed` is not true | no `verification`: `missing` ("not yet verified") |
| same | `plan-<id>` | every step passed, and there is at least one | any step failed | a step not yet verified, or the plan has no steps: `missing` |
| same | `constraint-<id>` | the row exists and `active` is not false (missing `active` counts as true) | `active` is false | no such row: `missing` |
| same | `failure-<id>` | the row exists | this selector does not fail | no such row: `missing` |
| `chisel:.chisel/store` | `test:<test_id>` | the latest `test_results` row has `passed` true | the latest row has `passed` false | no row: `missing` |
| same | `file:<path>` | at least one `test_edges` row points at the file, and every such test last passed | any such test's latest result failed | a touching test has no result, or no edge touches the file: `missing` |
| `probatio:<out-dir>` | none, or `survivors` | at least one `killed`, zero `survived`, no `flaky` or `error`, and a result for every mutant in the run's list | any result is `survived` | only `timeout`, `no coverage`, or `unviable`; any `flaky` or `error`; fewer results than the run's list; a `run.json` that cannot be read; or no results file and only a tally: `inconclusive`. An outcome Probatio does not write: `unreadable` |
| same | `mutant:<id>` | `results/<id>.json` has outcome `killed` | outcome `survived` | another Probatio outcome: `inconclusive`. An outcome Probatio does not write: `unreadable` |
| same | `ledger` | never | never | `inconclusive`, with the entry count |

`killed` with `cause` `build` is `unviable`. It is not a kill and not a survivor.
The outcomes Probatio writes are `killed`, `survived`, `timeout`,
`no coverage`, `unviable`, `flaky`, and `error`, matched exactly. When
`<out-dir>/run.json` names its patches (`patches`, a path or the first of a
list) inside the project, the mutant list there (`mutants.json` in a folder,
or the file itself) is compared with `results/`, as Probatio's tally does. A
list outside the project, or no `run.json`, is not checked.

A `file:` path matches a `code_id` when the code id is exactly that path, or
starts with `path:` or `path/`. `src/a.rs` does not match `src/a.rs2:...`.
The latest Chisel result is the last row in the snapshot. `tests/a.rs::name`
and `tests/a.rs:name` are the same id.

When the pointer names the root store, however it is spelt (`./`, `//`, `.`,
or a symlinked folder, compared by resolved path), and `.chisel/shards/`
exists, each
shard snapshot (including `_unassigned`) is read too. A pointer at one shard
reads only that file. A pointer under `.chisel/overlays/` is `no_adapter`:
worktree overlays are not read. A damaged shard makes the root pointer
`unreadable`.

## 4. How the other files are read

Adapters only read. They do not run the other tools, and they do not create
lock files, delete `store.tmp`, or delete a journal.

**Trammel.** The live document is `.trammel/store` plus `.trammel/journal`.
Each non-empty journal line must be JSON with `schema_version`. The last such
line replaces the snapshot. A snapshot that is not valid JSON is `unreadable`,
not an empty plan list, even if a journal sits beside it. After the journal,
`schema_version` must be 1. If `.trammel/lock` exists, the reader takes a
shared lock and does not wait: a held lock is `unreadable`. The lock and the
journal are looked for beside the store path as the pointer names it, not
beside a symlink's target.

**Chisel.** `.chisel/store` must have `"magic": "chisel-store"` and
`"version": 1`. `test_edges` and `blame` may be stored as compact tables
(`{"table":1,"cols":[...],"dict":[...],"strings":[...],"rows":[...]}`). Those
expand to plain rows. A compact table that cannot be expanded is `unreadable`.
If `.chisel/chisel.lock` exists, the reader takes a shared lock the same way.

**Probatio.** `results/*.json` is the source of truth. One file that is not
valid JSON, or that has no `outcome`, makes the whole-run pointer
`unreadable`. Files that are not `.json` are ignored, including `*.json.tmp`.
`tally.json` is a cache. When both exist and the survivor counts disagree,
the results stand and the detail says so. If only `tally.json` exists, the
pointer is `inconclusive` and says that results were missing. `#ledger` reads
`ledger.json`, counts `entries`, and is not proof.

**SQLite.** A file that starts with `SQLite format 3` is `unreadable`. For
Trammel and Chisel the detail says to run that tool's `migrate`. There is no
SQLite reader. The live stores are JSON.

**Freshness.** Applied only to a pointer that would otherwise `pass`:

- A timestamp (`verification.at` or `recorded_at`) more than a second older
  than the claim's `created` becomes `stale`, and the claim is unverified.
- A `commit` on the evidence (or, for Chisel, `test_passes` for that test)
  is compared with `git rev-parse HEAD`. A mismatch is `stale`, and the detail
  names both commits. A prefix of at least 7 hex digits matches. If git is
  missing, or the folder is not a repository, the pointer is `stale` rather
  than verified.
- No timestamp and no commit can still pass. The detail says `no timestamp`.
- A `fail` stays a `fail`.

## 5. Verdict

Per pointer, `read_evidence` returns a state:

| State | Meaning |
|---|---|
| `pass` | The evidence says the claim holds. |
| `fail` | The evidence says it does not. |
| `missing` | The file or the row is not there, or a step has not been verified yet. |
| `unreadable` | Outside the project, the wrong kind of file, over 64 MiB, not UTF-8, a symlink route out, SQLite, bad JSON, bad magic, or a selector the adapter rejects. |
| `no_adapter` | Recognised, and not interpreted. Worktree overlays. Not proof. |
| `inconclusive` | Read, and neither a pass nor a fail. A mutation timeout, a run with no kill and no survivor, an unchecked ledger. |
| `stale` | Was a pass, and is older than the claim or tied to a commit that is not HEAD. |

Per claim, `combine`:

- **refuted** if any pointer is `fail`
- **verified** if there is at least one pointer and every one is `pass`
- **unverified** otherwise, including no pointers

The reason for each pointer is in the `detail` of the view. A damaged claim
file fails the whole view and the whole list. It is not left out.

## 6. MCP tools

- `adsertio_claim` `{summary, evidence?, author?, body?, id?, tags?}` returns
  `{id, evidence}`. Without `id`, one is made from the summary. Never overwrites.
- `adsertio_attach` `{id, evidence}` adds pointers to an existing claim and
  returns `{id, evidence, added}`. `evidence` is still the new count. `added`
  is the pointers this call wrote, in order. A bad pointer, an empty list, or a
  duplicate (in the request, or already on the claim) is an error and writes
  nothing. Only that claim file is replaced. A claim holds at most 50 pointers.
  Attach will not go past that.
- `adsertio_check` `{}` is the same read as `adsertio check`. It fails on a
  damaged claim, a file in `claims` that is not `slug.md`, a `created` time
  that is not `YYYY-MM-DDTHH:MM:SSZ`, or a pass whose timestamp or commit is
  malformed. It reads each evidence file once per call and does not run git.
  An attach lock or temp file whose writer is gone is removed. A live writer
  is left alone, and check does not fail because of that lock.
- `adsertio_view` `{id?, verdict?, states?, compact?, limit?, offset?, after?}`
  returns `{claims, counts, counts_complete, truncated, next_offset, next_after, hint?}`.
  Each claim has `id`, `summary`, `verdict`, and `evidence` (`pointer`, `state`,
  and `detail` unless `compact` is true). `states` is a list of `verified`,
  `refuted`, and `unverified`. Pass `verdict` or `states`, not both. `counts`
  tallies the claims this call judged. `counts_complete` is true when that is
  every claim in the scope: the one `id`, or the whole store when `after` is
  omitted and the call reads through to the end. A page stops once it has
  `limit` matches. It may judge one further claim only to see that another
  page exists, and that claim is not added to `counts`.
- Claims are ordered by `created`, then by id, with number runs compared as
  integers (`c2` before `c10`). `after` is the `created/id` value in
  `next_after`, split at the last `/`. `created` is matched as stored, so a
  cursor from a hand-written time that `check` reports still works. Times
  have one-second resolution: a claim created later in the same second as
  the cursor, with an id that sorts before it, is not reached by that cursor. The next page starts strictly after that claim, so a verdict
  that changes on an earlier claim does not skip a later one. `offset` still
  skips that many matches from the start of this call and can skip a claim
  when a verdict changes between pages. When `truncated` is true, `hint` names
  the next call and uses `after`, repeating `id`, `verdict`, `states`, and
  `compact` when they were set. `next_offset` is set only when the call did
  not pass `after`. A list of more than 100 claims, with no limit, also
  includes `hint`.
- `adsertio_ls` `{limit?, offset?}` returns `{claims, count, truncated, next_offset, hint?}`.
  Each claim has `id`, `summary`, and `pointers`. It does not read evidence and
  does not include a verdict. `count` is the whole store. With no `limit` and
  no `offset`, `adsertio_ls` returns every claim. `limit` is 1 to 500.
- With no `limit`, `adsertio_view` returns every matching claim in its scope.
  `limit` is 1 to 500 when it is set.
- `adsertio_attach` locks that one claim for the read and the write. The lock
  file starts with the holder pid. A pid that is not running is removed at
  once. Text that is not a pid is removed once the file is a second old. A
  mtime more than a couple of seconds in the future, with no live pid, is
  removed at once. A couple of seconds ahead is treated as clock skew while
  the pid line is written. A live pid is kept: the call waits 5 seconds and
  then fails, and the file is unchanged. A future mtime does not keep a dead
  lock. Only the holder of the `<lock>.break` guard removes a dead lock, after
  judging it again while holding the guard, so a lock taken a moment later is
  never removed. A holder removes its lock only while the file still holds
  the line it wrote.
- A pointer path with a backslash, a drive letter, a leading slash, or `..` is
  an error at `adsertio_claim` and `adsertio_attach`. Use a relative path with
  forward slashes inside the project.
- `adsertio_guide` `{}` returns pointer shapes, the verdict rule, paging, and
  what to call after compaction. It does not read the store.

One `view` or `check` call reads a given Trammel snapshot, Chisel snapshot, or
Probatio `results/` directory once, indexes the rows a selector needs, and
reuses that index for every claim in the call. A paged `view` judges only the
claims on that page.

Arguments are checked against the schema: unknown arguments, wrong types,
missing required arguments, and values outside an `enum` are errors with
`isError: true`. Group: `core` only.

## 7. Protocol behaviour

Newline-delimited JSON-RPC 2.0 on stdio.

| Input | Reply |
|---|---|
| Invalid UTF-8, malformed JSON, nesting deeper than 64 | `-32700`, server continues |
| A line over 4 MiB | `-32600`, the line is skipped without being buffered |
| A batch (array) or a non-object | `-32600` |
| `id` that is not a string, number, or null | `-32600` |
| Unknown method with an id | `-32601` |
| Unknown method without an id, or `notifications/*` | no reply |
| `tools/call` for an unknown tool | `-32602` |
| A tool failure | a result with `isError: true` |
| A tool that panics | a result with `isError: true`; the server continues |

## 8. Not in this version

- Worktree overlays under `.chisel/overlays/`.
- A verdict that ledger bugs were re-caught. `#ledger` only counts entries.
- Resolving `adsertio:<id>` from a Libellus note. Libellus already stores the
  pointer as text.
- Running Trammel, Chisel, or Probatio.
- An npm package or a crates.io publish. A tag named `v*` builds release binaries and attaches them to the GitHub release.
