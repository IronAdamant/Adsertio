# Changelog

## 1.1.0

Faster repeated evidence reads, and a smaller answer for an agent.

- A `view` parses a shared Trammel snapshot, Chisel snapshot, or Probatio `results/` directory once and reuses it for every claim in that call. A regular Probatio result file is read in place. A symlink still has to stay inside the project. See `docs/perf/baseline-1.0.0.md` and `docs/perf/1.1.0.md`.
- `adsertio_ls` and `adsertio_view` take `limit` (default 100, maximum 500) and `offset`. The page is `claims`. `truncated` and `next_offset` say whether another page exists. `count` and `counts` still cover every claim. Callers that listed a store of 100 claims or fewer see the same claims as 1.0.0, plus the two page fields.
- `adsertio_guide` returns pointer shapes, the verdict rule, paging, and what to call after compaction. It does not read the store.
- A missing claim id says to call `adsertio_ls` or `adsertio_claim`.

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
