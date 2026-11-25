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
    instructions: "Adsertio keeps claims and pointers to their evidence. Nothing counts as verified without evidence that passes. Record all claims in one adsertio_claim call (claims: [{summary, evidence}]); a pointer is tool:path#selector into Trammel, Chisel or Probatio files (relative path, forward slashes). Add pointers with adsertio_attach (items: [{id, evidence}]). Read verdicts with adsertio_view. adsertio_guide has pointer shapes and what to do after compaction; adsertio_doctor finds problems.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
