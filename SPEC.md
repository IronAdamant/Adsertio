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
- A symlink in the claims folder, or any file that is not `<id>.md`, is an error.

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
| `probatio:<out-dir>` | none, or `survivors` | at least one `killed` and zero `survived` | any result is `survived` | only `timeout`, `no coverage`, or `unviable`, or no results file and only a tally: `inconclusive` |
| same | `mutant:<id>` | `results/<id>.json` has outcome `killed` | outcome `survived` | any other outcome: `inconclusive` |
| same | `ledger` | never | never | `inconclusive`, with the entry count |

`killed` with `cause` `build` is `unviable`. It is not a kill and not a survivor.

A `file:` path matches a `code_id` when the code id is exactly that path, or
starts with `path:` or `path/`. `src/a.rs` does not match `src/a.rs2:...`.
The latest Chisel result is the last row in the snapshot. `tests/a.rs::name`
and `tests/a.rs:name` are the same id.

When the pointer is exactly `.chisel/store` and `.chisel/shards/` exists, each
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
shared lock and does not wait: a held lock is `unreadable`.

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
  returns `{id, evidence}` (the new count). A bad pointer, an empty list, or a
  duplicate (in the request, or already on the claim) is an error and writes
  nothing. Only that claim file is replaced.
- `adsertio_view` `{id?, verdict?}` returns `{claims, counts}`. Each claim has
  `id`, `summary`, `verdict`, and `evidence` (`pointer`, `state`, `detail`).
  `counts` covers every claim in the read, whatever `verdict` filters the list to.
- `adsertio_ls` `{}` returns `{claims, count}`. Each claim has `id`, `summary`,
  and `pointers`. It does not read evidence and does not include a verdict.

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
- Release binaries and npm packages.
