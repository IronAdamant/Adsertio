//! Adsertio: one verified-or-unverified view per claim, read from the evidence
//! that Trammel, Chisel and Probatio left on disk.
//! Standard library only. See basis.md for the design and SPEC.md for the format.

pub mod cache;
pub mod chisel;
pub mod chisel_idx;
pub mod claim;
pub mod cli;
pub mod doctor;
pub mod fresh;
pub mod fsx;
pub mod groups;
pub mod hook;
pub mod json;
pub mod lock;
pub mod mcp;
pub mod paging;
pub mod probatio;
pub mod proc;
pub mod read;
pub mod store;
pub mod tool;
pub mod tools;
pub mod trammel;
pub mod trammel_idx;
pub mod verdict;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub static SERVER: mcp::Server = mcp::Server {
    name: "adsertio",
    version: VERSION,
    instructions: "Adsertio keeps claims and the evidence behind them. Record claims with adsertio_claim and point at proof in Trammel, Chisel or Probatio (tool:path#selector, a relative path with forward slashes inside the project). Record all claims in one call: pass claims, a list of {summary, evidence}. Add pointers with adsertio_attach (items: [{id, evidence}] for several claims in one call). List claims with adsertio_ls and read verdicts with adsertio_view. Omit limit to get every claim; pass limit (at most 500) and follow hint, which passes after, when the page is truncated. Call adsertio_guide for pointer shapes and what to do after the conversation is compacted. If something looks wrong, call adsertio_doctor. Nothing counts as verified without evidence that passes.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
