## Adsertio: claims and the evidence behind them

This project uses Adsertio (`.adsertio/`). A claim is one line saying something
is true, with pointers to proof that Trammel, Chisel or Probatio left on disk.

- Record a claim with `adsertio_claim`. Point at evidence as `tool:path`, for
  example `trammel:.trammel/store`. Do not paste results in.
- Call `adsertio_view` to see each claim as verified, refuted or unverified.
- Do not tell anyone a claim is verified unless `adsertio_view` says so. A claim
  with no evidence, missing evidence, or evidence marked `no_adapter` is
  unverified.

Choose tool groups with a line in the `agent-tools` block of AGENTS.md:

````markdown
```agent-tools
adsertio: tools core
```
````
