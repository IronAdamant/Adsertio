//! A claim: one line saying something is true ("the retry change passes its
//! plan step"), plus pointers to the evidence that would prove it. The
//! verdict is never stored. It is worked out from the evidence every time, so
//! it cannot go stale.
//!
//! Parsing is strict, like Libellus and Relatum notes: an unknown or duplicate
//! key, a bad pointer, or a missing field is an error naming the file. A
//! damaged claim is never skipped.

use crate::fsx;
use crate::store::Store;
use std::fs;

pub const EVIDENCE_TOOLS: &[&str] = &["trammel", "chisel", "probatio"];
pub const MAX_SUMMARY: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub created: String,
    pub evidence: Vec<String>,
    pub tags: Vec<String>,
    pub body: String,
}

/// `<tool>:<path>[#selector]`, the tool one of `EVIDENCE_TOOLS`, the path
/// relative to the project. No spaces, commas or brackets.
pub fn valid_pointer(p: &str) -> Result<(), String> {
    let (tool, rest) = p
        .split_once(':')
        .ok_or_else(|| format!("evidence \"{}\" must look like tool:path", fsx::clip(p, 60)))?;
    if !EVIDENCE_TOOLS.contains(&tool) {
        return Err(format!(
            "evidence tool \"{tool}\" is not one of: {}",
            EVIDENCE_TOOLS.join(", ")
        ));
    }
    let path = rest.split('#').next().unwrap_or("");
    if path.is_empty()
        || p.chars().count() > 300
        || p.chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, ',' | '[' | ']'))
    {
        return Err(format!(
            "evidence \"{}\" has an empty path, or a space, comma or bracket",
            fsx::clip(p, 60)
        ));
    }
    Ok(())
}

pub fn validate_summary(s: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err("summary is empty".into());
    }
    if s.contains('\n') || s.contains('\r') {
        return Err("summary must be one line".into());
    }
    if s.chars().count() > MAX_SUMMARY {
        return Err(format!(
            "summary is longer than {MAX_SUMMARY} characters; put the detail in the body"
        ));
    }
    if s != s.trim() {
        return Err("summary has leading or trailing spaces".into());
    }
    Ok(())
}

impl Claim {
    pub fn render(&self) -> String {
        let mut s = format!(
            "---\nsummary: {}\nauthor: {}\ncreated: {}\n",
            self.summary, self.author, self.created
        );
        if !self.evidence.is_empty() {
            s.push_str(&format!("evidence: [{}]\n", self.evidence.join(", ")));
        }
        if !self.tags.is_empty() {
            s.push_str(&format!("tags: [{}]\n", self.tags.join(", ")));
        }
        s.push_str("---\n");
        if !self.body.is_empty() {
            s.push('\n');
            s.push_str(self.body.trim_end_matches('\n'));
            s.push('\n');
        }
        s
    }

    pub fn parse(id: &str, text: &str) -> Result<Claim, String> {
        let mut lines = text.split('\n');
        if lines.next() != Some("---") {
            return Err("does not start with a --- frontmatter line".into());
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut closed = false;
        let mut consumed = 4;
        for line in lines.by_ref() {
            consumed += line.len() + 1;
            if line == "---" {
                closed = true;
                break;
            }
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| format!("bad frontmatter line \"{}\"", fsx::clip(line, 60)))?;
            let (k, v) = (k.trim(), v.trim());
            if !["summary", "author", "created", "evidence", "tags"].contains(&k) {
                return Err(format!("unknown frontmatter key \"{k}\""));
            }
            if fields.iter().any(|(fk, _)| fk == k) {
                return Err(format!("duplicate frontmatter key \"{k}\""));
            }
            fields.push((k.to_string(), v.to_string()));
        }
        if !closed {
            return Err("frontmatter is never closed with ---".into());
        }
        let get = |k: &str| {
            fields
                .iter()
                .find(|(fk, _)| fk == k)
                .map(|(_, v)| v.clone())
        };
        let need = |k: &str| {
            get(k)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("missing frontmatter field \"{k}\""))
        };
        let summary = need("summary")?;
        validate_summary(&summary)?;
        let list = |k: &str| -> Result<Vec<String>, String> {
            match get(k) {
                None => Ok(Vec::new()),
                Some(v) => {
                    let inner = v
                        .strip_prefix('[')
                        .and_then(|r| r.strip_suffix(']'))
                        .ok_or_else(|| format!("{k} must be written as [a, b]"))?;
                    Ok(inner
                        .split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(String::from)
                        .collect())
                }
            }
        };
        let evidence = list("evidence")?;
        for p in &evidence {
            valid_pointer(p)?;
        }
        let tags = list("tags")?;
        if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
            return Err(format!("bad tag \"{bad}\""));
        }
        let body = text[consumed.min(text.len())..]
            .trim_matches('\n')
            .to_string();
        Ok(Claim {
            id: id.to_string(),
            summary,
            author: need("author")?,
            created: need("created")?,
            evidence,
            tags,
            body,
        })
    }
}

/// Every claim, oldest first. A damaged claim fails the whole read.
pub fn load_all(store: &Store) -> Result<Vec<Claim>, String> {
    let dir = store.claims_dir();
    let rd = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut claims = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue; // a write in flight
        }
        let path = e.path();
        let ft = e
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
        if ft.is_symlink() {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                path.display()
            ));
        }
        let Some(id) = name.strip_suffix(".md").filter(|_| ft.is_file()) else {
            return Err(format!(
                "unexpected entry {} in the claims folder",
                path.display()
            ));
        };
        let text = fsx::read_strict(&path)?;
        claims.push(
            Claim::parse(id, &text)
                .map_err(|e| format!("corrupt claim {id} ({}): {e}", path.display()))?,
        );
    }
    claims.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Claim {
        Claim {
            id: "retry-passes".into(),
            summary: "The retry change passes its plan step".into(),
            author: "main".into(),
            created: "2026-11-25T00:00:00Z".into(),
            evidence: vec![
                "trammel:.trammel/store#step-2".into(),
                "chisel:.chisel/store".into(),
            ],
            tags: vec!["net".into()],
            body: "Why this matters.".into(),
        }
    }

    #[test]
    fn round_trip_and_strictness() {
        let c = sample();
        assert_eq!(Claim::parse("retry-passes", &c.render()).unwrap(), c);
        let ok = c.render();
        assert!(Claim::parse("x", &ok.replace("chisel:", "jest:")).is_err());
        assert!(Claim::parse("x", &ok.replace("author: main", "author: main\nauthor: x")).is_err());
        assert!(Claim::parse(
            "x",
            &ok.replace("author: main", "author: main\nverdict: verified")
        )
        .is_err());
        assert!(Claim::parse("x", "---\nsummary: s\n").is_err());
    }

    #[test]
    fn pointers() {
        assert!(valid_pointer("probatio:out/run.json#survivors").is_ok());
        assert!(valid_pointer("probatio:").is_err());
        assert!(valid_pointer("probatio:#x").is_err());
        assert!(valid_pointer("stele:a").is_err());
        assert!(valid_pointer("chisel:a b").is_err());
    }
}
