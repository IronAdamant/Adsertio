//! Turning evidence pointers into a verdict.
//!
//! v0.1 is the skeleton of this: it finds each evidence file inside the
//! project, refuses anything outside it, reads it strictly, and reports what
//! it found. It does NOT yet interpret Trammel, Chisel or Probatio output, so
//! a readable file is `NoAdapter`, which never counts as proof. The adapters
//! (one per tool) are the main work left; see SPEC.md.

use crate::claim::{self, Claim};
use crate::fsx;
use crate::json::Value;
use std::path::Path;

/// Evidence files larger than this are not read.
pub const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;

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
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Pass => "pass",
            State::Fail => "fail",
            State::Missing => "missing",
            State::Unreadable => "unreadable",
            State::NoAdapter => "no_adapter",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub pointer: String,
    pub state: State,
    pub detail: String,
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
    };
    if let Err(e) = claim::valid_pointer(pointer) {
        return mk(State::Unreadable, e);
    }
    let (tool, rest) = pointer.split_once(':').unwrap_or(("", pointer));
    let path = rest.split('#').next().unwrap_or("");
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
            format!("{path} is a folder; point at a file"),
        );
    }
    if meta.len() > MAX_EVIDENCE_BYTES {
        return mk(
            State::Unreadable,
            format!("{path} is larger than {MAX_EVIDENCE_BYTES} bytes"),
        );
    }
    let text = match fsx::read_strict(&file) {
        Ok(t) => t,
        Err(e) => return mk(State::Unreadable, e),
    };
    mk(
        State::NoAdapter,
        format!(
            "read {} bytes, but the {tool} adapter is not built yet, so this is not proof",
            text.len()
        ),
    )
}

pub struct View {
    pub verdict: Verdict,
    pub evidence: Vec<Evidence>,
}

pub fn view(project: &Path, claim: &Claim) -> View {
    let evidence: Vec<Evidence> = claim
        .evidence
        .iter()
        .map(|p| read_evidence(project, p))
        .collect();
    let states: Vec<State> = evidence.iter().map(|e| e.state).collect();
    View {
        verdict: combine(&states),
        evidence,
    }
}

impl View {
    pub fn to_value(&self, claim: &Claim) -> Value {
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
                            Value::obj(vec![
                                ("pointer", Value::str(&e.pointer)),
                                ("state", Value::str(e.state.name())),
                                ("detail", Value::str(&e.detail)),
                            ])
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
    }

    #[test]
    fn evidence_states_without_adapters() {
        let d = std::env::temp_dir().join(format!("adsertio-verdict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("ok.json"), "{}").unwrap();
        fs::write(d.join("bad.bin"), [0xffu8, 0xfe]).unwrap();
        assert_eq!(read_evidence(&d, "chisel:ok.json").state, State::NoAdapter);
        assert_eq!(read_evidence(&d, "chisel:nope.json").state, State::Missing);
        assert_eq!(read_evidence(&d, "chisel:../x").state, State::Unreadable);
        assert_eq!(read_evidence(&d, "chisel:sub").state, State::Unreadable);
        assert_eq!(read_evidence(&d, "chisel:bad.bin").state, State::Unreadable);
        assert_eq!(read_evidence(&d, "jest:ok.json").state, State::Unreadable);
    }
}
