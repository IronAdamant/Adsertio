# Changelog

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
