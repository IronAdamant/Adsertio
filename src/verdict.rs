//! Turning evidence pointers into a verdict.
//!
//! Each pointer is resolved inside the project. A SQLite file is unreadable
//! (the live stores are JSON; the old databases are for that tool's migrate
//! command). A readable file this version does not interpret yet is
//! `NoAdapter`, which never counts as proof.

use crate::claim::{self, Claim};
use crate::fsx;
use crate::json::Value;
use crate::read;
use std::path::Path;

pub use read::MAX_EVIDENCE_BYTES;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// The evidence says the claim holds.
    Pass,
    /// The evidence says the claim does not hold.
    Fail,
    /// The file the pointer names is not there.
    Missing,
    /// The file is outside the project, a folder, too big, not UTF-8, or damaged.
    Unreadable,
    /// The file reads fine but no adapter for its tool exists yet.
    NoAdapter,
    /// The file was read and it is neither a pass nor a fail: a mutation
    /// timeout, a run with no kill and no survivor, or an unchecked ledger.
    Inconclusive,
    /// A pass that is too old, tied to a commit that is not HEAD, or stamped
    /// with a timestamp or commit this version cannot read.
    /// Only a pass is downgraded; a failure stays a failure.
    Stale,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Pass => "pass",
            State::Fail => "fail",
            State::Missing => "missing",
            State::Unreadable => "unreadable",
            State::NoAdapter => "no_adapter",
            State::Inconclusive => "inconclusive",
            State::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub pointer: String,
    pub state: State,
    pub detail: String,
    /// Unix seconds on the evidence, when it names a time. The oldest one.
    pub at: Option<f64>,
    /// Commits the evidence names. Empty when it names none.
    pub commits: Vec<String>,
    /// Beside each of `commits`, the git tree the pass ran against when the
    /// tool recorded one (Chisel's `tree`). A pass whose tree is HEAD's tree
    /// tested exactly the committed code.
    pub trees: Vec<Option<String>>,
    /// The evidence named a timestamp or commit that is not a usable value.
    pub malformed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Verified,
    Refuted,
    Unverified,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::Refuted => "refuted",
            Verdict::Unverified => "unverified",
        }
    }
}

/// Verified only when there is evidence and all of it passes. Refuted when
/// any of it fails. Everything else, including no evidence, is unverified.
pub fn combine(states: &[State]) -> Verdict {
    if states.contains(&State::Fail) {
        Verdict::Refuted
    } else if !states.is_empty() && states.iter().all(|s| *s == State::Pass) {
        Verdict::Verified
    } else {
        Verdict::Unverified
    }
}

pub fn read_evidence(project: &Path, pointer: &str) -> Evidence {
    let mk = |state, detail: String| Evidence {
        pointer: pointer.to_string(),
        state,
        detail,
        at: None,
        commits: Vec::new(),
        trees: Vec::new(),
        malformed: false,
    };
    if let Err(e) = claim::valid_pointer(pointer) {
        return mk(State::Unreadable, e);
    }
    let (tool, path, _) = crate::selector::split(pointer);
    if tool == "trammel" {
        return crate::trammel::evaluate(project, pointer);
    }
    if tool == "chisel" {
        return crate::chisel::evaluate(project, pointer);
    }
    if tool == "probatio" {
        return crate::probatio::evaluate(project, pointer);
    }
    if tool == "relatum" {
        return crate::relatum::evaluate(project, pointer);
    }
    let file = match fsx::project_file(project, path) {
        Ok(f) => f,
        Err(e) if e.ends_with("does not exist") => return mk(State::Missing, e),
        Err(e) => return mk(State::Unreadable, e),
    };
    let meta = match std::fs::metadata(&file) {
        Ok(m) => m,
        Err(e) => return mk(State::Unreadable, format!("cannot stat {path}: {e}")),
    };
    if !meta.is_file() {
        return mk(
            State::Unreadable,
            format!("{path} {}", fsx::not_a_file(&meta)),
        );
    }
    if meta.len() > MAX_EVIDENCE_BYTES {
        return mk(
            State::Unreadable,
            read::size_detail(path, meta.len(), MAX_EVIDENCE_BYTES),
        );
    }
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => return mk(State::Unreadable, format!("cannot read {path}: {e}")),
    };
    if bytes.len() as u64 > MAX_EVIDENCE_BYTES {
        return mk(
            State::Unreadable,
            read::size_detail(path, bytes.len() as u64, MAX_EVIDENCE_BYTES),
        );
    }
    if read::is_sqlite(&bytes) {
        return mk(State::Unreadable, read::sqlite_message(tool));
    }
    if std::str::from_utf8(&bytes).is_err() {
        return mk(State::Unreadable, format!("{path} is not valid UTF-8"));
    }
    mk(
        State::NoAdapter,
        format!(
            "read {} bytes, but the {tool} adapter is not built yet, so this is not proof",
            bytes.len()
        ),
    )
}

pub struct View {
    pub verdict: Verdict,
    pub evidence: Vec<Evidence>,
}

pub fn view(project: &Path, claim: &Claim) -> View {
    let mut evidence: Vec<Evidence> = claim
        .evidence
        .iter()
        .map(|p| read_evidence(project, p))
        .collect();
    for item in &mut evidence {
        crate::fresh::apply(project, &claim.created, item);
    }
    let states: Vec<State> = evidence.iter().map(|e| e.state).collect();
    View {
        verdict: combine(&states),
        evidence,
    }
}

impl View {
    pub fn to_value(&self, claim: &Claim, compact: bool) -> Value {
        Value::obj(vec![
            ("id", Value::str(&claim.id)),
            ("summary", Value::str(&claim.summary)),
            ("verdict", Value::str(self.verdict.name())),
            (
                "evidence",
                Value::Array(
                    self.evidence
                        .iter()
                        .map(|e| {
                            let mut fields = vec![
                                ("pointer", Value::str(&e.pointer)),
                                ("state", Value::str(e.state.name())),
                            ];
                            if !compact {
                                fields.push(("detail", Value::str(&e.detail)));
                            }
                            Value::obj(fields)
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn combining_states() {
        assert_eq!(combine(&[]), Verdict::Unverified);
        assert_eq!(combine(&[State::Pass]), Verdict::Verified);
        assert_eq!(combine(&[State::Pass, State::Pass]), Verdict::Verified);
        assert_eq!(combine(&[State::Pass, State::Missing]), Verdict::Unverified);
        assert_eq!(
            combine(&[State::Pass, State::NoAdapter]),
            Verdict::Unverified
        );
        assert_eq!(
            combine(&[State::Pass, State::Fail, State::Missing]),
            Verdict::Refuted
        );
        assert_eq!(combine(&[State::Unreadable]), Verdict::Unverified);
        assert_eq!(combine(&[State::Inconclusive]), Verdict::Unverified);
        assert_eq!(
            combine(&[State::Pass, State::Inconclusive]),
            Verdict::Unverified
        );
        assert_eq!(
            combine(&[State::Fail, State::Inconclusive]),
            Verdict::Refuted
        );
        assert_eq!(combine(&[State::Stale]), Verdict::Unverified);
        assert_eq!(combine(&[State::Fail, State::Stale]), Verdict::Refuted);
    }

    #[test]
    fn evidence_states_without_adapters() {
        let d = std::env::temp_dir().join(format!("adsertio-verdict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("ok.json"), "{}").unwrap();
        fs::write(d.join("bad.bin"), [0xffu8, 0xfe]).unwrap();
        assert_eq!(read_evidence(&d, "chisel:ok.json").state, State::Unreadable);
        assert_eq!(
            read_evidence(&d, "probatio:ok.json").state,
            State::Unreadable
        );
        assert_eq!(read_evidence(&d, "chisel:nope.json").state, State::Missing);
        assert_eq!(read_evidence(&d, "chisel:../x").state, State::Unreadable);
        assert_eq!(read_evidence(&d, "chisel:sub").state, State::Unreadable);
        assert_eq!(read_evidence(&d, "chisel:bad.bin").state, State::Unreadable);
        assert_eq!(read_evidence(&d, "jest:ok.json").state, State::Unreadable);
        fs::write(d.join("old.db"), b"SQLite format 3\x00rest").unwrap();
        let trammel = read_evidence(&d, "trammel:old.db");
        assert_eq!(trammel.state, State::Unreadable);
        assert!(
            trammel.detail.contains("migrate") && trammel.detail.contains("trammel"),
            "{}",
            trammel.detail
        );
        let chisel = read_evidence(&d, "chisel:old.db");
        assert!(
            chisel.detail.contains("migrate") && chisel.detail.contains("chisel"),
            "{}",
            chisel.detail
        );
    }
}
