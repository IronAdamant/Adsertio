# Adsertio: LLM development log

## Project overview

**Adsertio** 1.2.0 is two binaries from one program (`adsertio`, `adsertio-mcp`) that give
one verified-or-unverified view per claim, read from the evidence Trammel, Chisel and
Probatio leave on disk. It does not call an LLM.

**Guidelines:** No third-party crates. `[dependencies]`, `[dev-dependencies]`, and
`[build-dependencies]` stay empty. No new TODO or FIXME markers in `src/`.

## Active context

- **Version:** 1.2.0. 1.0.0 and 1.1.1 stay the tags they are. 1.2.0 is the next tag. Not published to npm or crates.io.
- **1.2.0:** page-scoped views, indexed Trammel and Chisel lookups, a stable `after` cursor, and recovery of dead attach locks and temp files. See `docs/perf/1.2.0.md`.
- **Built:** claim, attach, view, ls; Trammel, Chisel, and Probatio readers;
  freshness; strict store; hostile-input tests.
- **Not built:** worktree overlays, a ledger re-catch verdict, Libellus id
  resolution, release binaries, npm. See SPEC.md section 8.
- **Next:** tag v1.2.0 on the merge commit. Do not move v1.0.0 or v1.1.1.

## Session log

---

## 1.2.0: fourth stress test

**Date:** 2026-11-25

- A paged view judges the page, not the whole store. `counts_complete` says whether the count is the whole scope.
- Trammel and Chisel rows are indexed once per snapshot per call. `check` uses that cache and does not run git.
- `after` is a `created/id` cursor. The hint uses it. `offset` remains.
- A dead attach pid, a future-mtime lock that is not live, and a temp file whose pid is gone are removed. A live pid is left for 5 seconds, then the attach fails.
- Re-measured the overload cases from the fourth stress test before and after. Numbers are in `docs/perf/1.2.0.md`.

---

## 1.1.0: repeated reads, guide, paging

**Date:** 2026-11-25

- Recorded a release-mode baseline, then kept two changes that measured faster:
  one parse of a shared snapshot per view, and a direct read of ordinary
  Probatio result files. Listing claim files did not get faster.
- `adsertio_guide`, and `limit` / `offset` on `ls` and `view`. Counts still
  cover the whole store. Omit limit to return every claim.

---

## 1.1.1: third review

**Date:** 2026-11-25

- Page hints keep the view filters. A symlinked claims folder is refused.
- Bad claim names are skipped with a warning and flagged by check. Bad created
  times and malformed evidence timestamps are stale, and check names them.
- Caps: 50 pointers, 20000 byte body, 262144 byte claim file. Auto ids retry.
  `adsertio_check` mirrors `adsertio check`. `jsonrpc` must be `2.0`.

---

## 1.1.0 review fixes

**Date:** 2026-11-25

- Attach takes an exclusive lock file so parallel updates are not dropped.
- The guide lists Probatio `#survivors`. Attach returns the pointers it wrote
  as well as the new evidence count.
- Paging applies only when limit or offset is passed.
- Pointers that cannot resolve are rejected at write. Duplicate ids use a
  relative path. Ids sort by number. A missing claims directory warns.

---

## 1.0.0: readers, freshness, attach, list

**Date:** 2026-11-25

- Read the on-disk formats from the public Trammel, Chisel, and Probatio
  repositories. Adapters read those files and do not run the tools.
- A shared lock is taken only when the lock file already exists. SQLite is
  refused. A damaged snapshot is not treated as empty.
- Trammel selectors cover a step, a plan, a constraint, and a failure. Chisel
  selectors cover a test and a file, including compact tables and shards.
  Probatio passes a run only when something was killed and nothing survived.
- A pass older than the claim, or tied to another commit, is `stale`. A failure
  still refutes. `git rev-parse HEAD` is the only external program.
- `adsertio_attach` adds pointers and writes nothing on a bad one.
  `adsertio_ls` does not read evidence and does not report a verdict.

---

## 0.1.0: scaffold

**Date:** 2026-11-25

- Started as the 9th tool, split from the notepad idea because it reads Trammel, Chisel
  and Probatio rather than holding notes. Built by Grok Bot from the design agreed with
  Aron, to be reviewed and finished in Claude Code.
- The foundations were written to avoid the failure modes found across the seven earlier
  tools. Readable evidence was `no_adapter`, never `pass`, until the adapters existed.
