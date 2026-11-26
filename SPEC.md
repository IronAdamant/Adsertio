# Adsertio format v1

Adsertio keeps claims and works out, on demand, whether the evidence behind
each one supports it. The claim files are the product. The CLI and MCP server
are conveniences over them. The evidence stays in the other tools' own files:
Trammel's, Chisel's and Probatio's records, and the rules Relatum keeps.

No claim is verified without a passing adapter result. A file the adapter
does not understand is `unreadable` or `no_adapter`, never `pass`.

## 1. Layout

```
.adsertio/
  claims/<id>.md        one claim per file; <id> is a slug
  locks/<id>.lock       that claim's attach lock (section 6), kept
```

- The store is the nearest `.adsertio/` walking up from the working directory,
  or the folder named by `$ADSERTIO_DIR`. `adsertio init` creates it. No tool
  creates a store by accident.
- The **project** is the folder that holds `.adsertio/`. Evidence paths are
  resolved inside it.
- `<id>` is a slug: `[a-z0-9][a-z0-9-]*`, at most 64 bytes, one segment.
- A symlink in the claims folder, or any file that is not `<id>.md`, is an
  error for `check`. `view` and `ls` leave it out and warn on stderr. This
  includes dotfiles. The only exceptions are Adsertio's own temp files
  (`.<name>.tmp-<pid>-<n>`), the lock files Adsertio 1.3 and earlier left
  there (`.<id>.md.lock` and `.<id>.md.lock.break`, see section 6), and macOS
  Finder's `.DS_Store`, which every command ignores. `.adsertio/locks/` is
  not read by `check`, `view` or `ls`.

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

- `tool` is `trammel`, `chisel`, `probatio`, or `relatum`.
- `path` is relative to the project, up to 300 characters in all, with no
  whitespace, commas, or brackets.
- `selector` chooses a row inside that file. The grammar is below.

Trammel and Chisel pointers with no selector are `unreadable` (a selector is
required) once the file itself can be read. A missing file is `missing`. A
Probatio pointer with no selector means the whole run. A Relatum pointer with
no note id, or a bad one, is `unreadable` whether or not the store is there.
Since 1.5.0 such pointers are refused when a claim is written or attached
(section 6), so they reach a view only from a claim file written by hand or
by an earlier version, and are read as above.

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
| `probatio:<out-dir>` | none, or `survivors` | at least one `killed`, zero `survived`, no `flaky`, `error`, or `no coverage`, and a result for every mutant in the run's lists | any result is `survived` | only `timeout` or `unviable`; any `flaky`, `error`, or `no coverage`; fewer results than the run's lists; a run whose lists cannot be found or read; a `mutate diff` run; or no results file and only a tally: `inconclusive`. An outcome Probatio does not write: `unreadable` |
| same | `mutant:<id>` | `results/<id>.json` has outcome `killed` | outcome `survived` | another Probatio outcome: `inconclusive`. An outcome Probatio does not write: `unreadable` |
| same | `ledger` | never | never | `inconclusive`, with the entry count |
| `relatum:<store>` (normally `.swarmnotes`) | `<note-id>` | the note exists, its frontmatter is sound, and `status` is `active` or absent | `status` is `superseded`; the detail names `superseded_by` and says to point the claim at it | `status` is `draft`: `inconclusive`. The store or the note file is not there: `missing`. No note id, a bad note id, damaged frontmatter (including a note that is not `superseded` but has a `superseded_by`, which `relatum get` also refuses), not UTF-8, over the size limit, or a path leaving the project: `unreadable` |

`killed` with `cause` `build` is `unviable`. It is not a kill and not a survivor.
The outcomes Probatio writes are `killed`, `survived`, `timeout`,
`no coverage`, `unviable`, `flaky`, and `error`, matched exactly.
`no coverage` blocks a pass: no test ran that line, and Probatio's own tally
says that is not a pass.

A whole-run pass needs the run to be complete. `<out-dir>/run.json` names its
patches (`patches`, a path or a list of them). Every entry is read the way
Probatio's `load_mutants` reads it: a file is a mutant list; a folder is its
`mutants.json`, or, when there is none, every `*.json` file in it. A relative
entry is taken from the project folder, where Probatio was run. Every mutant
id in every list must have a result in `results/`. When this cannot be
checked, the run is `inconclusive`, never a pass: no `run.json`, no patches
named, an entry outside the project or gone, a list that cannot be read, or
a list with no mutants.

A `mutate diff` run (its out folder holds `plan.json`, `generate/`, and the
run in `run/`) is always `inconclusive`. Probatio's `plan.json` holds only a
hash of the plan, and `generate/mutants.json` holds only the mutants sent to
the run so far, so a call capped by `--max-mutants` cannot be told from a
whole one. `#mutant:<id>` still reads one mutant of such a run.

A `file:` path matches a `code_id` when the code id is exactly that path, or
starts with `path:` or `path/`. `src/a.rs` does not match `src/a.rs2:...`.
The latest Chisel result is the last row in the root snapshot.
`tests/a.rs::name` and `tests/a.rs:name` are the same id.

Chisel records test results (`test_results` and `test_passes`) only in the
root store `.chisel/store`; a shard engine reads them from there. When the
pointer names the root store, however it is spelt (`./`, `//`, `.`, a
symlinked folder, or another letter case, compared by resolved path), and
`.chisel/shards/` exists, every shard snapshot under it (including
`_unassigned`, and nested ones such as `.chisel/shards/src/net/store`, since
Chisel names a shard after its path) adds its `test_edges`. A shard's own
`test_results` and `test_passes` are not read, so an old shard row never
overrides the root. A damaged shard makes the root pointer `unreadable`. A
pointer at one shard's store is `no_adapter`: its rows are not where Chisel
keeps results.

A pointer under `.chisel/overlays/` is `no_adapter`: worktree overlays are
not read. The parts are compared without regard to letter case, and the
path is also resolved, so `.CHISEL/OVERLAYS/...` or a symlink into the
overlays folder is caught.

A Relatum note id is the note's path under the store without `.md`: segments
matching `_?[a-z0-9][a-z0-9-]*`, joined by `/` (Relatum SPEC section 2). The
pointer reads `<store>/<note-id>.md`, nothing else. The supersede chain is not
followed: a pointer judges the note it names, so a superseded note fails even
when its replacement is active, and the claim should be pointed at the
replacement.

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

**Relatum.** The store is a folder; a file is `unreadable`. The note must
resolve inside the project and inside the store, so a symlink out of either
is `unreadable`, and a folder named like a note (`<id>.md/`) is `unreadable`.
A scope folder with no `<id>.md` is `missing`. The note is read the way
Relatum reads it: a leading byte order mark is dropped, and CRLF and CR
become LF (Relatum writes CRLF on Windows). The frontmatter opens with `---`
on the first line and closes at the first `\n---\n`. Blank lines and lines
starting with `#` are skipped. Every other line must be `key: value`; a
repeated key keeps its last value, as Relatum's reader does. The note is
damaged, and the pointer `unreadable`, when the frontmatter is missing or
never closed, a line is not `key: value`, `kind` or `summary` is missing or
empty, `status` is not `active`, `draft` or `superseded`, or `status` is
`superseded` and `superseded_by` is missing or not a note id, or `status` is
not `superseded` but `superseded_by` is set (not blank and not `[]`), which
Relatum's own `resolve` refuses. A value
written as an inline list (`[a, b]`) is not a kind, summary or status.
Other keys, including unknown ones, are not judged; that is `relatum
check`'s job. Relatum publishes each note whole (a hard link for a new note,
a rename for a supersede) and has no lock file, so Adsertio takes no lock
and never writes into the store.

**Size.** A Trammel or Chisel store, and the Trammel journal, may be up to
1 GiB, which is Chisel's own limit (`MAX_INPUT`), so any store Chisel writes
can be read. Any other evidence file, Probatio's and a Relatum note
included, may be up to 64 MiB. A larger file is `unreadable`, and the detail
gives its size, the limit, and why: the file is read whole into memory and
then indexed, so a read holds about the file's size plus that index.

**SQLite.** A file that starts with `SQLite format 3` is `unreadable`. For
Trammel and Chisel the detail says to run that tool's `migrate`. There is no
SQLite reader. The live stores are JSON.

**Freshness.** Applied only to a pointer that would otherwise `pass`:

- A timestamp (`verification.at` or `recorded_at`) more than a second older
  than the claim's `created` becomes `stale`, and the claim is unverified.
- A `commit` on the evidence (or, for Chisel, `test_passes` for that test)
  is compared with `git rev-parse HEAD`. Chisel writes an empty `commit` in
  `test_passes` when it could not read HEAD. That is no commit recorded, not
  a malformed one: no commit is checked, the timestamp rule still applies,
  `check` does not fail on it, and the detail says Chisel recorded no commit. A mismatch is `stale`, and the detail
  names both commits, then ends with a fix: `; fix: rerun the tests at HEAD
  (for example chisel run -- <test command>)` for Chisel, or `(for example
  verify_step again for that step)` for Trammel. This is the usual case after
  an agent tests uncommitted work and then commits. A prefix of at least 7 hex digits matches. If git is
  missing, or the folder is not a repository, the pointer is `stale` rather
  than verified.
- A Chisel pass may also carry `tree` (in `test_passes`, or on the
  `test_results` row when that row has the `commit`): the git tree id of the
  working tree the tests ran against, written by Chisel 0.25.0 and later. When
  the commit is not HEAD but `tree` equals `git rev-parse HEAD^{tree}`, the
  pass is fresh: the tested code is exactly the committed code. It is also
  fresh when `git diff-tree -r` between the two trees lists only paths under
  `.adsertio/`, `.chisel/`, `.trammel/` or `.probatio/` (claims and evidence
  written around the run and committed with it). The detail then says the
  pass was tested before the commit on that tree. Any other difference is
  `stale`, and the detail names both trees. A tree that cannot be compared
  (git does not have it) is `stale`. `tree` must be 40 or 64 hex digits; any
  other value, or no `tree`, leaves the commit rule above as it is, and the
  stale detail's fix adds that Chisel 0.25.0 or later records the tree.
  HEAD and its tree come from one `git rev-parse HEAD HEAD^{tree}` per
  project per view; `git diff-tree` runs at most once per recorded tree per
  view, and only when that tree is not HEAD's. Trammel records no tree, so
  a Trammel pass keeps the commit rule.
- No timestamp and no commit can still pass. The detail says `no timestamp`.
  Probatio records no time or commit, so a Probatio pass is never stale.
- A Relatum note is a rule: a state, not an event, like a Trammel
  `constraint-<id>`. A rule written before the claim is normal, so a Relatum
  pass carries no time and no commit and is never `stale`, whatever HEAD is
  and however old the note. The note's `updated` date is not read for
  freshness, and git is not run for it.
- A `fail` stays a `fail`.

## 5. Verdict

Per pointer, `read_evidence` returns a state:

| State | Meaning |
|---|---|
| `pass` | The evidence says the claim holds. |
| `fail` | The evidence says it does not. |
| `missing` | The file, the row, or the Relatum note is not there, or a step has not been verified yet. |
| `unreadable` | Outside the project, the wrong kind of file, over the size limit (below), not UTF-8, a symlink route out, SQLite, bad JSON, bad magic, damaged note frontmatter, or a selector the adapter rejects. |
| `no_adapter` | Recognised, and not interpreted. Worktree overlays. Not proof. |
| `inconclusive` | Read, and neither a pass nor a fail. A mutation timeout, a run with no kill and no survivor, an unchecked ledger, a Relatum draft. |
| `stale` | Was a pass, and is older than the claim or tied to a commit that is not HEAD (unless Chisel's recorded tree shows it tested the committed code). |

Per claim, `combine`:

- **refuted** if any pointer is `fail`
- **verified** if there is at least one pointer and every one is `pass`
- **unverified** otherwise, including no pointers

The reason for each pointer is in the `detail` of the view. A damaged claim
file fails the whole view and the whole list. It is not left out.

## 6. MCP tools

- `adsertio_claim` `{summary, evidence?, author?, body?, id?, tags?}` returns
  `{id, evidence}`. Without `id`, one is made from the summary. Never overwrites.
- `adsertio_claim` `{claims}` records several claims in one call and returns
  `{ids, count}`, the ids in the order of the items. `claims` is a list of 1
  to 50 objects, each with the fields of one claim (`summary` required;
  `evidence`, `id`, `author`, `body`, `tags` optional). Passing `claims` and
  any single-claim field together is an error. Semantics:
  1. Every item is checked before anything is written: its fields (an unknown
     field is an error), summary, pointers, author, tags, body, id, and size.
     An explicit id must not repeat another item's id and must not name a
     claim that exists. Any failure writes nothing, and the error names the
     item by number (`claims item 2: ...`) and says nothing was written.
  2. The items are then written in order, each with `overwrite = false`. An
     item without `id` takes its summary's slug, or the next free `slug-<n>`,
     skipping ids already on disk and ids other items use.
  3. If a write fails part way (another process created the same explicit id
     after the check), the call is an error that names the failed item and
     lists the ids written and the items not written. Nothing written is
     undone, and the items after the failure are not tried.
- `adsertio_attach` `{id, evidence}` adds pointers to an existing claim and
  returns `{id, evidence, added}`. `evidence` is still the new count. `added`
  is the pointers this call wrote, in order. A bad pointer, an empty list, or a
  duplicate (in the request, or already on the claim) is an error and writes
  nothing. Only that claim file is replaced. A claim holds at most 50 pointers.
  Attach will not go past that.
- `adsertio_attach` `{items}` attaches to several claims in one call and
  returns `{claims: [{id, evidence, added}]}`. `items` is a list of 1 to 50
  `{id, evidence}`, one item per claim (two items naming one claim is an
  error). Every item is checked against its claim first, as a single attach
  checks it; any failure writes nothing and names the item
  (`attach item 2: ...`). Then each claim is updated under its own lock, and
  checked again while locked. If one fails then (another attach added the
  same pointer in between), the error lists the claims updated and the ones
  not updated.
- The schema does not mark `summary` (claim) or `id` and `evidence` (attach)
  as required, since a batch call has none of them at the top level. A call
  with neither form is an error that names both.
- The CLI takes `--claims JSON` or `--claims-file PATH`, and `--items JSON` or
  `--items-file PATH`: the same list as JSON text, or a file holding it (up
  to 4 MiB, UTF-8, a leading byte order mark ignored). Giving both, or one
  twice, is a usage error (exit 2), as is text that is not a JSON array.
- `adsertio check` (CLI only; there is no MCP tool since 1.3.1, and
  `adsertio_doctor` reports the same problems) fails on a
  damaged claim, a file in `claims` that is not `slug.md`, a `created` time
  that is not `YYYY-MM-DDTHH:MM:SSZ`, or a pass whose timestamp or commit is
  malformed. It reads each evidence file once per call and does not run git.
  It first removes temp files in `claims` over an hour old and any lock file
  1.3 left there, and lists the temp files that remain in `stray_temp_files`
  (they do not fail it). It never deletes a file in `.adsertio/locks/`.
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
  integers (`c2` before `c10`). Ids whose parts are equal as numbers
  (`a01b1` and `a1b01`) are ordered by their bytes, so no two ids tie and a
  cursor never skips or repeats a claim. `after` is the `created/id` value in
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
- `adsertio_attach` locks that one claim for the read and the write: an
  operating-system lock on `.adsertio/locks/<id>.lock`, opened read-write
  (created when missing, 0600 on Unix, in a `locks` folder made 0700) and
  kept open while held. Unix takes `flock(fd, LOCK_EX | LOCK_NB)`; Windows
  takes `LockFileEx` with `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`
  on the whole file. A busy lock is retried with a sleep of 1 to 10 ms for up
  to 5 seconds; then the call fails with `this claim is being updated by
  another attach. Retry adsertio_attach; nothing was written.` and the claim
  is unchanged. The kernel releases the lock when the holder closes the file,
  exits or is killed, so there is no stale lock: no pid, age or token is
  written or judged. Each claim has its own lock, so attaches to different
  claims do not wait for each other. Lock files are empty and never deleted
  (removing a held one would let a second attach lock a new file at that
  name), and there is at most one per claim: an attach to an id with no
  claim file takes no lock and creates no file. A symlink at `locks` or at a
  lock file, or a `locks` that is not a folder, is an error. Locks are
  advisory: an edit by hand takes none. On some NFS and SMB set-ups `flock`
  is emulated or not shared between machines, so two attaches from
  different machines could both hold one; `doctor` warns (section 7).
- Files from 1.3: 1.3 locked with `.<id>.md.lock` in `claims` (a pid, a time
  and a token, created exclusive and deleted on release) and a
  `.<id>.md.lock.break` guard. 1.4 never uses those names, so one left in a
  store blocks nothing. `view` and `ls` skip them, `doctor` lists them under
  `leftover_files`, and `check` or `doctor --fix` deletes them. Running 1.3
  and 1.4 attaches on one store at the same time is not supported: they take
  different locks.
- Temp files (`.<name>.tmp-<pid>-<n>`) are judged by age only: one whose
  mtime is more than an hour old is abandoned, and `view`, `ls`, `check` and
  `doctor --fix` remove it. The pid in the name is not checked. A write
  holds its temp for milliseconds; a writer paused for longer only loses
  that write (its rename or link fails), since temp names are never reused.
- A pointer path with a backslash, a drive letter, a leading slash, or `..` is
  an error at `adsertio_claim` and `adsertio_attach`. Use a relative path with
  forward slashes inside the project.
- Write-time and view-time checks. `adsertio_claim` and `adsertio_attach`
  (CLI and MCP, single and batch) check each pointer's shape, with the same
  selector grammar the readers use (section 3, `src/selector.rs`), and refuse
  a pointer that can never be valid. A refused pointer writes nothing; in a
  batch the error names the item (`claims item 2: ...`, `attach item 2: ...`).
  At write time:
  - the pointer form and path rules above;
  - Trammel: a selector, one of `plan-<id>/step-<id>`, `plan-<id>`,
    `constraint-<id>`, `failure-<id>`, each id digits that fit a signed
    64-bit integer;
  - Chisel: a selector, `test:<test_id>` with a non-empty id, or
    `file:<path>` with a non-empty path and no `..` part;
  - Probatio: none, `survivors`, `ledger`, or `mutant:<id>` where `<id>` is
    one file name (non-empty, no `/`, `\` or `..`). `#ledger` is accepted:
    it is a valid shape that never passes;
  - Relatum: a note id after `#` (section 3), and a path that names the
    store folder: a path ending in `.md` (after any trailing `/`) is refused.

  Everything else is judged when the claim is viewed: whether the file, row,
  run or note is there (`missing`), whether it can be read (`unreadable`),
  and freshness (`stale`). A pointer already stored is read as it always was,
  so a stored pointer with a bad selector views as `unreadable`; there is no
  migration and the claim is not damaged.

  The error is one line: `evidence "<pointer>": <what is wrong>. Expected
  <shape>`, then `; did you mean <pointer>?` when the fix is plain from the
  text alone (no file is read). Relatum `<x>` with no `#`, `<x>` a note id,
  suggests `relatum:.swarmnotes#<x>`. A Relatum path with a `.swarmnotes`
  segment followed by a note id, with or without `.md` (`.swarmnotes/<id>.md`
  or `.swarmnotes/<id>` with no `#`), is split at that segment:
  `relatum:.swarmnotes#<id>`. A Chisel selector with no `test:` or `file:`
  suggests `file:<x>` when it holds a `/`, else `test:<x>`. A suggestion is
  offered only when it would itself be accepted.
- `adsertio_guide` `{}` returns pointer shapes, the verdict rule, paging, and
  what to call after compaction. It does not read the store.
- `adsertio_doctor` `{fix?}` returns `{ok, checks, fixed?}`. See section 7.

One `view` or `check` call reads a given Trammel snapshot, Chisel snapshot, or
Probatio `results/` directory once, indexes the rows a selector needs, and
reuses that index for every claim in the call. A paged `view` judges only the
claims on that page.

Arguments are checked against the schema: unknown arguments, wrong types,
missing required arguments, and values outside an `enum` are errors with
`isError: true`. Group: `core` only.

## 7. Hook and doctor

**`adsertio hook --host claude|cursor|generic`** (CLI only; default
`generic`) prints a snippet for a person to paste. It takes only `--host`,
under the same flag rules as every command: an unknown, repeated, or empty
flag, or an unknown host, is a usage error. Adsertio does not install the
snippet. Every snippet runs

```
adsertio view --compact --states unverified --states refuted --limit 50 --if-store
```

which lists only the claims that are not verified, so after compaction the
agent reads them again instead of repeating a verdict from memory.
`--if-store` (command line only) prints nothing and exits 0 when no
`.adsertio` folder is found and ADSERTIO_DIR is unset, so the hook can sit
in user settings for every project. The
command has no shell syntax (no `&&`, `||`, quotes, `~`, or variables).

- `claude`: JSON for `.claude/settings.json`. A `SessionStart` entry with
  matcher `startup|resume|clear|compact`, and a `SubagentStart` entry (matcher
  `*`) whose command adds `--wrap claude-subagent`. Both use the exec form,
  `"command": "adsertio"` with an `args` list, which runs the program with no
  shell, the same under cmd, PowerShell and Git Bash.
- `cursor`: JSON for `.cursor/hooks.json`, a `sessionStart` command that adds
  `--wrap cursor`. Cursor's `preCompact` cannot add text, so the snippet says
  to run the view again after compaction.
- `generic`: the command line alone.

`adsertio view --wrap cursor|claude-subagent` (CLI only) prints the view's
JSON as text inside the JSON the host reads: `{"additional_context": ...}`
for Cursor, and `{"hookSpecificOutput": {"hookEventName": "SubagentStart",
"additionalContext": ...}}` for a Claude Code sub-agent. The text starts with
one line saying these are the claims that are not verified.

**`adsertio doctor [--fix]`** and the MCP tool `adsertio_doctor` `{fix?}`
return `{ok, checks, fixed?}`. Each check is `{name, ok, detail, fix}`, and
`fix` is a sentence when the check fails, null when it passes. `ok` is true
when every check passes. The CLI prints the JSON either way and exits 0 when
`ok` is true, 1 when not. The MCP tool returns the report as a normal result
(`isError` false); `ok` carries the answer. The checks, in order:

| Name | Fails when |
|---|---|
| `version` | never; the version and platform |
| `store` | no store is found, or `ADSERTIO_DIR` names one that does not exist, or `claims` is not a real folder. The detail says whether `ADSERTIO_DIR` is set |
| `store_writable` | a probe file cannot be written with `fsx::write_atomic` (`overwrite = false`, a hard link, as a new claim is) and removed |
| `claims` | a claim is damaged, a file in `claims` is not `slug.md`, a `created` time is malformed, or a pass a pointer names has a malformed timestamp or commit (each evidence file is read once; git is not run) |
| `leftover_files` | `claims` holds a temp file over an hour old, or a `.<id>.md.lock` or `.<id>.md.lock.break` file from 1.3. Younger temp files are listed and pass. Files in `.adsertio/locks/` are never listed |
| `path_length` | Windows only: the claims folder path plus the longest name Adsertio writes there (104 characters) reaches 260 |
| `file_system` | never; it warns (passes with a `fix`) when the store is on a network file system, where file locks may be emulated or not shared between machines: on macOS a mount without `MNT_LOCAL` (autofs trigger points aside), on Linux x86_64 and aarch64 an `f_type` of NFS, SMB, CIFS, SMB2, 9P, AFS, Coda or Ceph, on Windows a UNC path or a drive `GetDriveTypeW` calls remote. Other platforms are not checked |
| `git` | `git` is not on `PATH`, or `git rev-parse HEAD` fails in the project |
| `trammel_store`, `chisel_store` | the file is over the 1 GiB limit, or present and not readable by Adsertio (read the way a pointer reads it, shared lock included) |
| `probatio` | `.probatio` is not a folder, or a run folder in it (one holding `results/`, `run.json`, `tally.json` or `ledger.json`, up to two levels down, at most 50) is `unreadable` |
| `relatum` | `.swarmnotes` is present and is not a folder inside the project, or a note in it (a `*.md` file whose path makes a note id, dot-names skipped, symlinked folders not followed, at most 2000) is `unreadable` read the way a pointer reads it. The detail counts the active, draft and superseded notes. Absent passes |
| `binaries` | `adsertio` and `adsertio-mcp` are not both in the folder of the running program |

Without a store, the store checks are left out and the rest run in the
working folder. Doctor reads only, except the probe file. `--fix` (or
`fix: true`) first removes the temp files over an hour old and the 1.3 lock
files in `claims`, using the same rules as `check`, then runs the
checks and lists the removed names in `fixed`. It changes nothing else.

## 8. Protocol behaviour

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

## 9. Not in this version

- Worktree overlays under `.chisel/overlays/`.
- A verdict that ledger bugs were re-caught. `#ledger` only counts entries.
- Resolving `adsertio:<id>` from a Libellus note. Libellus already stores the
  pointer as text.
- Running Trammel, Chisel, Probatio, or Relatum, or writing a Relatum note.
- Following a Relatum supersede chain, or judging a note's `updated` date.
- An npm package or a crates.io publish. A tag named `v*` builds release binaries and attaches them to the GitHub release.
