# Adsertio format v0.1

Adsertio keeps claims and works out, on demand, whether the evidence behind
each one supports it. The claim files are the product; the CLI and MCP server
are conveniences over them. The evidence stays in the other tools' own files.

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
evidence: [trammel:.trammel/store#plan-3/step-2, chisel:.chisel/store]
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
missing field or an unclosed frontmatter are errors naming the file.

**Evidence pointer:** `<tool>:<path>[#selector]`.

- `tool` is `trammel`, `chisel` or `probatio`.
- `path` is relative to the project, up to 300 characters in all, with no
  whitespace, commas or brackets.
- `selector` is for the adapter: which plan step, test, file or mutant inside the
  evidence. Its grammar belongs to each adapter and is not fixed yet.

## 3. Verdict

Per pointer, `read_evidence` returns a state:

| State | Meaning |
|---|---|
| `pass` | The evidence says the claim holds. (Adapters only; none yet.) |
| `fail` | The evidence says it does not. (Adapters only; none yet.) |
| `missing` | The file does not exist. |
| `unreadable` | Outside the project, a folder, over 64 MiB, not UTF-8, a symlink route out, or a bad pointer. |
| `no_adapter` | The file reads fine but nothing interprets it yet. Not proof. |

Per claim, `combine`:

- **refuted** if any pointer is `fail`;
- **verified** if there is at least one pointer and every one is `pass`;
- **unverified** otherwise, including no pointers.

The reason for each pointer is in the `detail` of the view. A damaged claim file
fails the whole view; it is not left out.

## 4. MCP tools (built)

- `adsertio_claim` `{summary, evidence?, author?, body?, id?, tags?}` returns
  `{id, evidence}`. Without `id`, one is made from the summary.
- `adsertio_view` `{id?, verdict?}` returns `{claims, counts}`. Each claim has
  `id`, `summary`, `verdict` and `evidence` (`pointer`, `state`, `detail`).
  `counts` always covers every claim, whatever `verdict` filters the list to.

Arguments are checked against the schema: unknown arguments, wrong types,
missing required arguments and values outside an `enum` are errors with
`isError: true`. Group: `core` only.

## 5. Protocol behaviour

Newline-delimited JSON-RPC 2.0 on stdio.

| Input | Reply |
|---|---|
| Invalid UTF-8, malformed JSON, nesting deeper than 64 | `-32700`, server continues |
| A line over 4 MiB | `-32600`, the line is skipped without being buffered |
| A batch (array) or a non-object | `-32600` |
| `id` that is not a string, number or null | `-32600` |
| Unknown method with an id | `-32601` |
| Unknown method without an id, or `notifications/*` | no reply |
| `tools/call` for an unknown tool | `-32602` |
| A tool failure | a result with `isError: true` |
| A tool that panics | a result with `isError: true`; the server continues |

## 6. Planned, not built

- **The adapters**, the main work. One per tool, each reading that tool's own
  files read-only and answering `pass`, `fail` or `unreadable` for a selector:
  - Trammel: `verify_step` results, constraints and failure history. Its store is
    `.trammel/store`; its on-disk format has to be confirmed from Trammel's source.
  - Chisel: test and coverage results from the `.chisel/store` snapshot (and
    shards under `.chisel/shards/`). Format to be confirmed from Chisel's source.
  - Probatio: mutation survivors from a run's output folder (`--out`) and its
    ledger. Format to be confirmed from Probatio's source.
  Each adapter must fail loudly on a file it does not understand: `unreadable`,
  never `pass`. Adapters read files; they never run the tools.
- **Freshness:** evidence older than the claim or than the current commit should
  not verify it. Needs a rule and a recorded commit.
- `adsertio_attach`: add evidence to an existing claim, replacing the file
  atomically.
- `adsertio_ls`: a cheap list without reading evidence.
- A link from Libellus: a note's evidence pointer `adsertio:<id>` already works as
  text; resolving it is left to a later version.
- Release binaries, npm packages, an MCP Registry entry.
