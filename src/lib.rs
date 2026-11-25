//! Adsertio: one verified-or-unverified view per claim, read from the evidence
//! that Trammel, Chisel and Probatio left on disk.
//! Standard library only. See basis.md for the design and SPEC.md for the format.

pub mod cache;
pub mod chisel;
pub mod claim;
pub mod cli;
pub mod fresh;
pub mod fsx;
pub mod groups;
pub mod json;
pub mod lock;
pub mod mcp;
pub mod probatio;
pub mod read;
pub mod store;
pub mod tool;
pub mod tools;
pub mod trammel;
pub mod verdict;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub static SERVER: mcp::Server = mcp::Server {
    name: "adsertio",
    version: VERSION,
    instructions: "Adsertio keeps claims and the evidence behind them. Record a claim with adsertio_claim and point at proof in Trammel, Chisel or Probatio (tool:path). Add a pointer with adsertio_attach. List claims without reading evidence with adsertio_ls. Call adsertio_view to see each claim as verified, refuted or unverified. Nothing counts as verified without evidence that passes; a claim with no evidence, missing evidence, or evidence Adsertio cannot read stays unverified, and the view says why.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
