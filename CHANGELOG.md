# Changelog

## 1.5.0

Same claim format as 1.4.0, with a fourth evidence tool. A project rule kept
as a Relatum note (a constraint, convention or decision in `.swarmnotes/`)
can now back a claim, and the verdict is worked out in Adsertio, so the CLI,
the MCP server and the session hook agree on it.

- `relatum:<store>#<note-id>`, the store relative to the project (normally
  `.swarmnotes`) and the note id its path under the store without `.md`. It
  passes when the note exists, its frontmatter is sound, and its `status` is
  `active` (or absent). A `superseded` note fails, and the detail names its
  `superseded_by` and says to point the claim there. A `draft` is
  `inconclusive`. A missing store or note is `missing`. No note id, a bad id,
  damaged frontmatter (missing or unclosed, a line that is not `key: value`,
  no `kind` or `summary`, an unknown `status`, `superseded` with no valid
  `superseded_by`, or an `active` or `draft` note that has a `superseded_by`,
  which Relatum's own `get` refuses), a note that is not UTF-8 or over 64 MiB, or a path or
  symlink leaving the project or the store is `unreadable`.
- The note is read the way Relatum reads it: a byte order mark is dropped and
  CRLF becomes LF, so a note Relatum wrote on Windows reads the same.
- A Relatum pass is never `stale`. A rule is a state, not an event, like a
  Trammel `constraint-<id>`: it is normal for it to predate the claim.
- The supersede chain is not followed: a pointer judges the note it names.
  Adsertio never writes into `.swarmnotes` and takes no lock there (Relatum
  publishes each note whole and has no lock file).
- `adsertio doctor` has a `relatum` check: `.swarmnotes` absent, present, and
  whether every note in it can be read, with a count of active, draft and
  superseded notes.
- `adsertio_guide` lists `relatum:.swarmnotes#<note-id>` and has a `relatum`
  field. The `adsertio_claim` and `adsertio_doctor` descriptions, the server
  instructions and `adsertio --help` name Relatum.
- The error for a pointer over 300 characters now says so.
- A pointer whose shape can never be valid is refused when a claim is written
  or attached (CLI and MCP, single and batch; a batch writes nothing and the
  error names the item), instead of being stored and judged `unreadable` at
  every view: a Trammel or Chisel pointer with no selector or an unknown one,
  an empty or bad id, a bad Probatio selector, a Relatum pointer with no note
  id or a bad one, and a Relatum path ending in `.md` (the note file, not the
  store folder). Claims cannot be overwritten and pointers cannot be removed,
  so a bad first pointer used to leave a claim unverified for good. Whether
  the evidence is there, and freshness, are still judged at view. Pointers
  already stored are read as before (no migration).
- The error is one line with the pointer, the expected shape and, when the
  text makes it plain, the fix: `relatum:_shared/no-deps` and
  `relatum:.swarmnotes/_shared/no-deps.md` both get `did you mean
  relatum:.swarmnotes#_shared/no-deps?`, and `chisel:.chisel/store#t1` gets
  `did you mean chisel:.chisel/store#test:t1?`.
- The selector grammar is defined once, in `src/selector.rs`. The Trammel,
  Chisel, Probatio and Relatum readers and the write-time check all call it,
  and `adsertio_guide`'s `selectors` list comes from it.
- `tests/pointer_shapes.rs`: a table of accepted and refused selectors per
  tool that the writer and the reader must agree on, claim and attach through
  the CLI and MCP, batch all-or-nothing, the suggestions word for word, a
  store left unchanged on refusal, and a stored bad pointer still read as
  `unreadable`.
- `tests/relatum.rs`: every state, the CLI and MCP end to end, mixed claims,
  hostile paths, bytes that are not a note, freshness, and doctor.

## 1.4.0

Same claim format as 1.3.1. This release cuts the rerun an agent made after
every commit: it tests uncommitted work, records claims, commits, and every
Chisel pass went stale because HEAD moved.

- A Chisel pass whose commit is not HEAD is fresh when the `tree` Chisel
  recorded (Chisel 0.25.0 and later: the git tree of the working tree the
  tests ran against, uncommitted and untracked files included) equals
  `HEAD^{tree}`, or differs from it only under `.adsertio/`, `.chisel/`,
  `.trammel/` or `.probatio/`. Committing exactly the tested work, with or
  without the claims and the Chisel store, keeps the pass. Any other change
  committed after the run is `stale`, and the detail names both trees.
  Without `tree` (older Chisel, Trammel) the commit rule is unchanged.
- HEAD and its tree come from one `git rev-parse HEAD HEAD^{tree}` per view,
  so the common case runs no extra git. `git diff-tree` runs once per
  recorded tree that is not HEAD's tree, cached for the view. A tree id goes
  to git only after it is checked to be 40 or 64 hex digits.
- The stale detail's fix for a Chisel pass with no recorded tree adds that
  Chisel 0.25.0 or later records the tested tree, so committing exactly that
  work needs no rerun. `adsertio_guide`'s `stale_after_commit` says the same.
- `tests/v140.rs` uses real Chisel output (`tests/fixtures/chisel/real-tree`):
  a dirty `chisel run -- cargo test` on a crate with a new test, committed
  exactly, committed with claims and store, and changed after the run.

Operating-system attach locks. An attach lock was `.<id>.md.lock`, made
with `create_new` and holding a pid, a time and a token, so an attach that
was killed left it behind and Adsertio had to guess when it was stale: a
dead pid (which pid reuse and separate pid namespaces get wrong), no pid
after one second, a mtime in the future, its own pid after 30 seconds, and a
`.break` guard so only one waiter removed it. Now the lock is `flock` on
Unix and `LockFileEx` on Windows, on a lock file kept open while held. The
kernel releases it when the holder exits or is killed, so a killed attach
blocks nobody and there is nothing to guess.

- Each claim has its own lock file, `.adsertio/locks/<id>.lock`, created by
  the first attach to that claim and never deleted (deleting a held lock
  file lets a second attach lock a new file at that name). Per-claim files
  keep attaches to different claims running side by side, as before; there
  is at most one file per claim, since an attach to an id with no claim
  file takes no lock. One store-wide lock would have meant one file, but
  would make every attach wait for every other. `check` and `doctor` do not
  look in `locks/`. A symlinked `locks` folder is refused.
- A `.<id>.md.lock` or `.<id>.md.lock.break` left by 1.3 blocks nothing.
  `view` and `ls` skip it, `doctor` lists it under `leftover_files`, and
  `check` and `doctor --fix` delete it. Running 1.3 and 1.4 attaches on one
  store at the same time is not supported: they take different locks.
- The busy error is unchanged: after 5 seconds, `this claim is being
  updated by another attach. Retry adsertio_attach; nothing was written.`
- Temp files are judged by age only: one over an hour old is removed. The
  pid in a temp name is no longer checked, and a temp with a mtime in the
  future is no longer removed at once. No pid is checked anywhere.
- `doctor` has a `file_system` check on every platform, which warns (and
  passes, with a `fix`) when the store is on a network file system, where
  `flock` may be emulated or not shared between machines: a macOS mount
  without `MNT_LOCAL`, a Linux NFS, SMB, CIFS, 9P, AFS, Coda or Ceph
  `f_type` (x86_64 and aarch64), or a Windows UNC path or remote drive. It
  replaces the Windows-only `network_drive` check, which failed.
- `src/oslock.rs` is Libellus 1.4.0's lock module, std only. On Windows,
  `src/lock.rs` (the shared locks on Trammel's and Chisel's lock files,
  which behave as before) now uses its `LockFileEx` declarations.
- Removed: `fsx::break_abandoned_lock`, `fsx::lock_held`,
  `fsx::lock_file_is_stale`, the `.break` guard, the lock token, the Windows
  access-denied retry when creating the lock, and `src/proc.rs`
  (`pid_alive`, `kill(pid, 0)`, `OpenProcess`).
- `tests/oslock.rs`: a second attach waits for a held lock and both pointers
  are kept, an attach to another claim does not wait, a holder killed with
  SIGKILL (TerminateProcess on Windows) frees the lock at once, 1.3 lock
  files block nothing and `check` removes them, and temp files go by age.

## 1.3.1

- `adsertio view --if-store` (CLI only) prints nothing and exits 0 when no
  `.adsertio` folder is found, so a session hook set for every project stays
  quiet where Adsertio is not used. With ADSERTIO_DIR set, a missing store is
  still an error. `adsertio hook` now adds it.
- Smaller tool definitions. Every agent turn reads `tools/list` and the
  server instructions again, so they cost on every call. The `tools` array
  went from 5,588 to 3,788 bytes (compact JSON) and `instructions` from 765
  to 487. Descriptions are short and say what to call when; detail lives in
  `adsertio_guide`, SPEC and README. Batch item schemas (`claims`, `items`)
  now give each field's type and the required fields, with no per-field
  descriptions. `states` items carry an `enum`, and `limit` a `maximum` of
  500. `tests/tool_budget.rs` keeps the array at or under 4,000 bytes and
  the instructions under 500.
- Removed the `adsertio_check` MCP tool; `adsertio_doctor` covers it. Its
  `claims` check now also reports a pass with a malformed timestamp or
  commit, as `check` does, and `fix: true` clears leftover temp and lock
  files. The `adsertio check` command is unchanged, for scripts and CI.
  `tools/list` has six tools.

## 1.3.0

Same claim format as 1.2.2. This release cuts agent turns: each MCP call is
one turn, and each turn reads the whole context again, so claims and attaches
now go in one call. It also adds a session hook and a doctor for set-up
problems, with Windows in mind.

Added:

- Batch claims. `adsertio_claim` takes `claims`, a list of up to 50 objects
  with the fields of one claim. Every item is checked first (fields, ids,
  repeated ids in the batch, ids already on disk, pointers, sizes); one bad
  item writes nothing and the error names it. Items are then written with
  `overwrite = false`; if a write fails part way, the error lists exactly
  which ids were written and which items were not. The reply is
  `{ids, count}`. CLI: `adsertio claim --claims '<json array>'` or
  `--claims-file PATH` (a byte order mark from Windows PowerShell is
  ignored).
- Batch attach. `adsertio_attach` takes `items`, a list of `{id, evidence}`,
  with the same all-or-nothing check. Each claim is updated under its own
  lock. CLI: `--items` or `--items-file`.
- The tool descriptions, the server instructions, and `adsertio_guide` tell
  an agent to record all claims in one call.
- `adsertio hook --host claude|cursor|generic` prints a hook that runs
  `adsertio view --compact --states unverified --states refuted --limit 50 --if-store`
  at session start and after compaction, so the agent sees the claims that are not
  verified instead of repeating verdicts from memory. The Claude Code
  snippet uses the exec form (`command` plus `args`, no shell), so it runs
  the same on Windows, and adds a `SubagentStart` entry for sub-agents.
- `adsertio view --wrap cursor|claude-subagent` (CLI only) prints the view
  inside the JSON those hooks read.
- `adsertio doctor [--fix]` and the `adsertio_doctor` tool: read-only checks
  of the version and platform, the store and `ADSERTIO_DIR`, a writable
  claims folder, damaged claims, files left by a dead writer, git and HEAD,
  the Trammel, Chisel and Probatio files (present, size, readable), the
  `adsertio-mcp` binary next to `adsertio`, and on Windows the path length
  and network drives. Each failing check has a `fix`. Exit 1 when a check
  fails. `--fix` removes only what `check` already removes.
- A Chisel or Trammel pass that is stale only because its commit is not
  HEAD now ends its detail with a fix: rerun the tests at HEAD (for example
  `chisel run -- <test command>`). This happens when an agent tests
  uncommitted work and then commits.

Fixed:

- On Windows, two attaches racing on one claim could fail with "cannot lock
  the claim: Access is denied". Windows reports that while another process
  is deleting the lock file. It now counts as a held lock and is retried.

Breaking changes (there were no users yet):

- The `adsertio_claim` schema no longer marks `summary` as required, and the
  `adsertio_attach` schema no longer marks `id` and `evidence` as required,
  because a batch call has none of them at the top level. A call with
  neither form is still an error, with a message naming both.
- `tools/list` has a seventh tool, `adsertio_doctor`, in the `core` group.
- The stale detail for a commit that is not HEAD has a new suffix,
  `; fix: rerun the tests at HEAD (...)`.
- `adsertio_guide` has new fields (`stale_after_commit`, `problems`), and its
  `after_compaction` and `writes` text changed.
- A single `adsertio_claim` with no arguments now says to pass `summary` or
  `claims`, instead of only `missing required argument: summary`.

## 1.2.2

Same claim format as 1.2.1. The rule for this release: when Adsertio cannot
show that a run is complete, or that a result is the latest, the answer is
`inconclusive` (or `no_adapter`), never `pass`. A failure still refutes.
Several pointers that 1.2.1 called pass are now inconclusive, on purpose.

- A Probatio `mutate diff` call capped by `--max-mutants` read verified: its
  `generate/mutants.json` lists only the batch sent to the run, so every
  mutant in it had a result. Probatio does not write the full list a diff
  plans (`plan.json` holds only a hash), so a capped call cannot be told from
  a whole one. A `mutate diff` run is now always `inconclusive`. Point at
  `#mutant:<id>`, or use `mutate generate` and `mutate run` for a whole run.
- A Probatio whole-run pointer passed when the run's mutant list could not be
  checked: no `run.json`, a list outside the project, a deleted patches
  folder, or a patches folder with one file per mutant and no `mutants.json`.
  Each of these is now `inconclusive`. A folder without `mutants.json` is
  read like Probatio's `load_mutants` reads it (every `*.json` in it), every
  `patches` entry is checked, not only the first, and a relative entry such
  as `.probatio/slice` is taken from the project folder.
- A `no coverage` mutant now blocks a Probatio pass (`inconclusive`), since
  Probatio's own tally says it is not a pass. A run with a kill and a
  `no coverage` result was verified in 1.2.1 and is unverified now.
- A Chisel shard's `test_results` row could override a newer root result.
  Chisel records results only in the root store (a shard engine reads them
  from there), so shard result rows and `test_passes` are no longer read; the
  shards still add their test edges. Nested shards such as
  `.chisel/shards/src/net/store` (Chisel names a shard after its path) are
  now found. A pointer at one shard's store is `no_adapter`. SPEC no longer
  claims the old merge matched Chisel.
- The overlay guard compared path parts exactly, so `.CHISEL/OVERLAYS/...` on
  a case-insensitive disk, or a symlink into `.chisel/overlays/`, was read.
  Parts are compared without regard to letter case, and the resolved path is
  checked too.
- Paging: ids whose number runs are equal (`a01b1`, `a1b01`) compared equal,
  so `--after` could skip one created in the same second. Ties now fall back
  to the bytes, and the order is total.
- Command line: a repeated `--limit`, `--offset`, `--compact` (any flag but
  `--evidence`, `--tags`, `--states`) silently took the first value; it is
  now a usage error. An unknown flag exits 2 (usage error), not 1. A value
  that starts with `--` is passed as `--name=value` or `--name -- value`.
- A symlinked `claims/<x>.md` made `view` and `ls` fail. It is left out with
  a warning, as SPEC says, and `check` reports it. `check` now also reports a
  dotfile in the claims folder that is not one of Adsertio's own temp, lock,
  or guard files, and `view` and `ls` warn about it. macOS Finder's
  `.DS_Store` is ignored.
- Chisel writes `test_passes[id].commit = ""` when it cannot read HEAD. That
  was treated as malformed (stale, and `check` exited 1). It is now no
  commit recorded: no commit is checked, the timestamp rule still applies,
  and the detail says so.
- A Trammel or Chisel store over 64 MiB was unreadable, though Chisel writes
  stores up to 1 GiB. The limit for those stores (and the Trammel journal)
  is now 1 GiB, Chisel's own limit; other evidence files keep 64 MiB. The
  unreadable detail gives the size, the limit, and why (the file is read
  whole into memory). A read no longer copies the bytes a second time.
- A `created` time with a `+` in a number field (`+026-...`) was accepted.
  It is rejected.
- AGENTS.md lists every test file; the Probatio adapter's header now
  describes what the code does.

## 1.2.1

Same claim format as 1.2.0. Fixes from the fifth review. A few runs that 1.2.0
called verified are now unverified, which is the point.

- A Chisel root store pointer spelt `.chisel/./store`, `.chisel//store`, or
  through a symlinked folder skipped the shards, so a failing shard result was
  hidden and the claim could read verified. The root is recognised by its
  resolved path.
- A Probatio run passed with `flaky` or `error` results, with an outcome
  Probatio does not write (`Survived`, `survived `), or with only some of its
  mutants finished. `flaky` and `error` now make the run inconclusive, an
  unknown outcome makes it unreadable, and when `run.json` names a mutant list
  inside the project, a pass needs a result for every mutant in it.
- Breaking a dead attach lock could let two attaches in at once, and the
  holder's cleanup could delete the next holder's lock. Only the holder of a
  `.break` guard removes a dead lock now, and a holder removes only the lock
  line it wrote. `check` removes dead locks the same way.
- `ADSERTIO_DIR=.adsertio` (one relative part) made every pointer
  unreadable. The project is the working folder in that case.
- A claim with a hand-written `created` time handed out a `next_after` cursor
  that `after` refused. The cursor now matches `created` as stored.
- The Trammel lock and journal were looked for beside a symlinked store's
  target instead of beside `.trammel/store`, so a newer journal could be
  missed. They are read beside the path as written.
- A Chisel `file:` detail listed every waiting or failed test (hundreds on a
  real store). It lists ten and says how many more.
- SPEC now says what the code does with a stray file in the claims folder:
  `view` and `ls` warn and leave it out, and `check` fails.

## 1.2.0

The fourth stress test. A page no longer walks every claim against every evidence row. A Trammel or Chisel snapshot is indexed once per call. On this machine a 20,000-claim page that took 249 s in 1.1.1 took 0.48 s, and the full view of that store took 0.50 s. RSS for the 14.9 MiB plus 11.5 MiB pair dropped from 238 MiB to 72 MiB. The re-measure, including what did not get cheaper, is in `docs/perf/1.2.0.md`.

- `adsertio_view` takes `after`, the `created/id` cursor from `next_after`. Claims stay ordered by `created`, then by id. The hint uses `after`, so a verdict that changes behind the cursor does not skip a later claim. `offset` still skips a count of matches and can skip one when a verdict changes. `next_offset` is set only when the call did not pass `after`.
- A page judges the claims it returns, plus non-matches and `offset` skips on the way there. It does not judge the rest of the store. `counts` covers the claims that were judged. `counts_complete` is true when that is the whole scope. Omit `limit` to judge every matching claim and get a complete count.
- `check` uses the same per-call evidence cache as `view`. It checks malformed timestamps and commits and does not run git. One snapshot is still read once, not once per claim.
- An attach lock starts with the holder pid. A pid that is not running is removed at once. Text that is not a pid is removed once the file is a second old, or at once when the mtime is in the future. A live pid is kept: the call waits 5 seconds, then fails, and the claim file is unchanged. A future mtime does not keep a dead lock.
- A temp file under `.adsertio/claims` whose pid is not running is removed on the next read, attach, or check. A live writer's temp is left in place, and `check` still lists it.

## 1.1.1

The third review. Correctness only. A re-measure is in `docs/perf/1.1.1.md`; it is not a speedup.

- A truncated `adsertio_ls` or `adsertio_view` hint repeats `id`, `verdict`, `states`, and `compact` along with `limit` and `offset`.
- `.adsertio/claims` as a symlink is refused. Claim files that are not `slug.md` are left out of `ls` and `view`, with a warning, and `check` names them.
- `check` rejects a `created` time that is not `YYYY-MM-DDTHH:MM:SSZ`. A pass whose `at` or `recorded_at` is not a number, or whose `commit` is empty, is `stale` and `check` names it.
- A named pipe is described as a pipe. A claim holds at most 50 pointers, a body of 20000 bytes, and a file of 262144 bytes. `attach` will not cross the pointer cap. Duplicate tags and evidence on a new claim are stored once.
- Claims that share a summary and omit `id` retry until each one is stored. `check` names a stale attach lock and says to remove it, then retry `adsertio_attach`.
- `limit` errors say the range is 1 to 500. A request whose `jsonrpc` is missing or not `2.0` is an error. `adsertio_claim` on a folder with no claims store says to run `init`. `adsertio_check` is the MCP form of `adsertio check`.
- Windows release zips are checked for a PK header, forward-slash names, and exactly `adsertio.exe` and `adsertio-mcp.exe` in the versioned folder.

## 1.1.0

Faster repeated evidence reads, and a smaller answer for an agent.

- A `view` parses a shared Trammel snapshot, Chisel snapshot, or Probatio `results/` directory once and reuses it for every claim in that call. A regular Probatio result file is read in place. A symlink still has to stay inside the project. See `docs/perf/baseline-1.0.0.md` and `docs/perf/1.1.0.md`.
- Omit `limit` and `offset` and `adsertio_ls` and `adsertio_view` return every matching claim, as 1.0.0 did. Pass `limit` (1 to 500) and `offset` to page. A truncated page includes `hint` with the next call. A list of more than 100 claims, with no limit, includes a hint that paging exists. `count` and `counts` still cover the whole read.
- `adsertio_view` takes `states` (verified, refuted, unverified) and `compact` (pointer and state, no detail). `verdict` still selects one state. Passing both `verdict` and `states` is an error.
- `adsertio_attach` holds an exclusive lock on that claim. Every successful attach is in the file. If another attach still holds the lock, the call fails and writes nothing. A lock older than 30 seconds is removed. The reply still has `evidence` as the new count, and also `added`, the pointers this call wrote.
- A pointer with a backslash, a drive letter, an absolute path, or `..` is rejected when it is written. The error says to use a relative path with forward slashes inside the project.
- A duplicate claim id names `.adsertio/claims/<id>.md`.
- `limit` has schema minimum 1.
- `ADSERTIO_DIR` set to a folder with no `claims` directory prints `no claims store found at X; run init` and still exits 0. An initialised empty store stays quiet.
- Claim ids that share a `created` time sort by number, so `c2` comes before `c100`.
- `adsertio_guide` returns pointer shapes, the verdict rule, paging, and what to call after compaction. It does not read the store. Probatio `#survivors` is listed, including the example `probatio:out/run#survivors`. It is the same whole-run check as a pointer with no selector.
- A missing claim id says to call `adsertio_ls` or `adsertio_claim`.
- The next GitHub release names archives `adsertio-<version>-<target-triple>` (for example `adsertio-1.1.0-x86_64-unknown-linux-gnu.tar.gz`). Windows zip entries use forward slashes. The v1.0.0 assets keep the names already published.

## 1.0.0

The readers, freshness, attach, and list. Release binaries for Linux x64, macOS x64, macOS arm64, and Windows x64 are attached to the GitHub release. Not published to npm or crates.io.

- Trammel adapter: `plan-<id>/step-<id>`, `plan-<id>`, `constraint-<id>`,
  `failure-<id>`. A shared lock on `.trammel/lock` when that file exists. The
  journal replaces the snapshot. A damaged snapshot is `unreadable`, not an
  empty plan list. A step with no `verification` is not yet verified.
- Chisel adapter: `test:<id>` uses the latest `test_results` row. `file:<path>`
  uses `test_edges`, including compact tables. The root `.chisel/store` also
  reads shards. Overlays are `no_adapter`. A shared lock on `chisel.lock` when
  that file exists.
- Probatio adapter: a run passes only when at least one mutant was killed and
  none survived. A survivor refutes. `timeout`, `no coverage`, and `unviable`
  are inconclusive. One broken `results/*.json` makes the run `unreadable`.
  `tally.json` is a cache and loses to `results/`. `#ledger` counts entries
  and is not proof. `#mutant:<id>` judges that one file.
- A file that starts with the SQLite header is `unreadable`, with a migrate
  hint for Trammel and Chisel. There is no SQLite reader.
- Freshness: a pass older than the claim, or tied to a commit that is not
  `HEAD`, is `stale` and the claim stays unverified. A failure still refutes.
  No timestamp and no commit can still pass, and the detail says so. The only
  external program is `git rev-parse HEAD`.
- `adsertio_attach` adds pointers and writes nothing when a pointer is bad or
  repeated. `adsertio_ls` lists id, summary, and pointer count, with no verdict.
  A damaged claim still fails the list.
- No claim is verified without a passing adapter result.

## 0.1.0

The scaffold.

- Strict, bounded JSON parser: nesting capped at 64, duplicate keys, control
  characters and lone surrogates rejected.
- Stdio MCP server: invalid UTF-8, malformed or deeply nested JSON, oversized
  lines and batches each get a JSON-RPC error; unknown methods are errors; a
  failed tool call sets `isError`; a panicking tool is contained.
- Tools: `adsertio_claim`, `adsertio_view`. Unknown arguments are errors.
- Claims in plain files with strict frontmatter; the verdict is computed from the
  evidence on every view and never stored.
- Evidence pointers are resolved inside the project; paths that leave it, folders,
  oversized and non-UTF-8 files are `unreadable`.
- Damaged claims fail the view loudly.
- Atomic writes with temp-file cleanup; claims are created with a hard link and
  never overwrite.
- Tool groups chosen in the AGENTS.md `agent-tools` block; a mistake falls back
  to the core tools and says why.
- CLI: `init`, `claim`, `view`, `check`, `mcp`.
