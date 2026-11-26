## Adsertio: claims and the evidence behind them

This project uses Adsertio (`.adsertio/`). A claim is one line saying something
is true, with pointers to proof that Trammel, Chisel or Probatio left on disk,
or to a project rule Relatum keeps (`relatum:.swarmnotes#<note-id>`).

- Record a claim with `adsertio_claim`. Point at evidence as `tool:path#selector`,
  for example `trammel:.trammel/store#plan-1/step-1`. Do not paste results in.
- Add a pointer later with `adsertio_attach`. A bad pointer is rejected.
- Call `adsertio_view` to see each claim as verified, refuted or unverified.
- `adsertio_ls` lists claims without reading the evidence and does not report a
  verdict. With no `limit`, `adsertio_ls` and `adsertio_view` return every
  matching claim. Pass `limit` (maximum 500) to page, and for a view follow
  `hint`, which passes `after`. `count` on `adsertio_ls` is the whole store.
  `counts` on a view page covers the claims that page judged.
- Call `adsertio_guide` for pointer shapes and for what to do after the
  conversation is compacted. Do not repeat a verdict from memory.
- Do not tell anyone a claim is verified unless `adsertio_view` says so. A claim
  with no evidence, missing evidence, or evidence marked `unreadable`,
  `no_adapter`, `inconclusive`, or `stale` is unverified.

Choose tool groups with a line in the `agent-tools` block of AGENTS.md:

````markdown
```agent-tools
adsertio: tools core
```
````
