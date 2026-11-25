# Adsertio: LLM development log

## Project overview

**Adsertio** 0.1.0 is a std-only Rust binary (`adsertio`, `adsertio-mcp`) that gives
one verified-or-unverified view per claim, read from the evidence Trammel, Chisel and
Probatio leave on disk. It does not call an LLM.

**Guidelines:** No third-party crates. `[dependencies]` stays empty. No new TODO or
FIXME markers in `src/`.

## Active context

- **Version:** 0.1.0, a scaffold. Nothing is tagged or published.
- **Built:** claim, view; strict store; verdict rules; hostile-input tests.
- **Not built:** the three adapters. Until they exist no claim can be verified.
- **Next:** the Trammel, Chisel and Probatio adapters, confirmed against each tool's
  source; freshness; attach. See SPEC.md section 6.

## Session log

---

## 0.1.0: scaffold

**Date:** 2026-11-25

- Started as the 9th tool, split from the notepad idea because it reads Trammel, Chisel
  and Probatio rather than holding notes. Built by Grok Bot from the design agreed with
  Aron, to be reviewed and finished in Claude Code.
- The foundations were written to avoid the failure modes found across the seven earlier
  tools. Readable evidence is `no_adapter`, never `pass`.
