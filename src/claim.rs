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

pub const EVIDENCE_TOOLS: &[&str] = &["trammel", "chisel", "probatio", "relatum"];
pub const MAX_SUMMARY: usize = 200;
pub const MAX_POINTERS: usize = 50;
pub const MAX_BODY_BYTES: usize = 20_000;
pub const MAX_CLAIM_BYTES: u64 = 256 * 1024;

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
            "evidence \"{}\" is longer than 300 characters, or has an empty path, or a space, comma or bracket",
            fsx::clip(p, 60)
        ));
    }
    if let Some(why) = pointer_path_problem(path) {
        return Err(format!(
            "evidence \"{}\" {why} Use a relative path with forward slashes inside the project, such as trammel:.trammel/store#plan-1/step-1",
            fsx::clip(p, 60)
        ));
    }
    Ok(())
}

/// Backslashes, drive letters, absolute paths, and `..` never resolve. Reject
/// them when the pointer is written, not only when it is read.
fn pointer_path_problem(path: &str) -> Option<&'static str> {
    let bytes = path.as_bytes();
    if path.contains('\\') {
        return Some("uses backslashes.");
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some("uses a drive letter.");
    }
    if path.starts_with('/') {
        return Some("is absolute.");
    }
    if path.split('/').any(|part| part == "..") {
        return Some("leaves the project with '..'.");
    }
    None
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

/// `created/id`, the cursor `view --after` passes back.
pub fn cursor(claim: &Claim) -> String {
    format!("{}/{}", claim.created, claim.id)
}

/// `true` when `claim` sorts strictly after `(created, id)`.
pub fn is_after(claim: &Claim, created: &str, id: &str) -> bool {
    match claim.created.as_str().cmp(created) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => cmp_id(&claim.id, id) == std::cmp::Ordering::Greater,
    }
}

/// Compare ids with number runs as integers, so `c2` comes before `c10`.
/// Ids that differ only in leading zeros are ordered by their bytes.
fn cmp_id(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if left[i].is_ascii_digit() && right[j].is_ascii_digit() {
            while i < left.len() && left[i] == b'0' {
                i += 1;
            }
            while j < right.len() && right[j] == b'0' {
                j += 1;
            }
            let i0 = i;
            let j0 = j;
            while i < left.len() && left[i].is_ascii_digit() {
                i += 1;
            }
            while j < right.len() && right[j].is_ascii_digit() {
                j += 1;
            }
            let order = (i - i0).cmp(&(j - j0)).then(left[i0..i].cmp(&right[j0..j]));
            if order != std::cmp::Ordering::Equal {
                return order;
            }
        } else {
            let i0 = i;
            let j0 = j;
            while i < left.len() && !left[i].is_ascii_digit() {
                i += 1;
            }
            while j < right.len() && !right[j].is_ascii_digit() {
                j += 1;
            }
            let order = left[i0..i].cmp(&right[j0..j]);
            if order != std::cmp::Ordering::Equal {
                return order;
            }
        }
    }
    // The shorter run of parts sorts first. When the parts are equal
    // (`a01b1` and `a1b01`), the bytes decide, so different ids never
    // compare equal and the order is total: a cursor never skips one.
    let left_more = i < left.len();
    let right_more = j < right.len();
    left_more.cmp(&right_more).then_with(|| left.cmp(right))
}

/// Every claim, oldest first. A damaged claim fails the whole read.
pub fn load_all(store: &Store) -> Result<Vec<Claim>, String> {
    load_all_with(store, true)
}

/// [`load_all`], and with `sweep` false it removes nothing: `doctor` without
/// `--fix` only reads.
pub fn load_all_with(store: &Store, sweep: bool) -> Result<Vec<Claim>, String> {
    let dir = store.claims_dir();
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(".adsertio/claims is a symlink; refusing to follow it. Run `adsertio init` so claims is a real folder inside the project.".into());
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(".adsertio/claims is not a folder. Run `adsertio init`.".into());
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no claims store found at {}; run init", store.dir.display());
            return Ok(Vec::new());
        }
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    }
    if sweep {
        fsx::sweep_abandoned_temps(&dir);
    }
    let rd = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut claims = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let name = e.file_name().to_string_lossy().to_string();
        if fsx::is_work_file(&name) || fsx::is_os_file(&name) {
            continue; // a write or an attach in flight, or a Finder file
        }
        if name.starts_with('.') {
            eprintln!(
                "skipping .adsertio/claims/{name}: not a claim file. adsertio check reports it."
            );
            continue;
        }
        let path = e.path();
        let ft = e
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
        if ft.is_symlink() {
            eprintln!(
                "skipping .adsertio/claims/{name}: a symlink is not followed. adsertio check reports it."
            );
            continue;
        }
        let Some(id) = name.strip_suffix(".md").filter(|_| ft.is_file()) else {
            eprintln!(
                "skipping .adsertio/claims/{name}: not a claim file. adsertio check reports it."
            );
            continue;
        };
        if !fsx::valid_slug(id) {
            eprintln!(
                "skipping .adsertio/claims/{name}: \"{id}\" is not a claim id. Use a slug. adsertio check reports it."
            );
            continue;
        }
        let len = e
            .metadata()
            .map_err(|err| format!("cannot stat {}: {err}", path.display()))?
            .len();
        if len > MAX_CLAIM_BYTES {
            return Err(format!("claim {id} is larger than {MAX_CLAIM_BYTES} bytes"));
        }
        let text = fsx::read_strict(&path)?;
        claims.push(
            Claim::parse(id, &text)
                .map_err(|e| format!("corrupt claim {id} ({}): {e}", path.display()))?,
        );
    }
    claims.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| cmp_id(&a.id, &b.id)));
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
        for bad in [
            r"trammel:.trammel\store",
            "trammel:C:/store",
            "trammel:/etc/passwd",
            "trammel:../secret",
        ] {
            let err = valid_pointer(bad).unwrap_err();
            assert!(err.contains("forward slashes"), "{bad}: {err}");
        }
    }

    #[test]
    fn ids_sort_numbers_ahead_of_text_order() {
        assert_eq!(cmp_id("c2", "c10"), std::cmp::Ordering::Less);
        assert_eq!(cmp_id("c10", "c100"), std::cmp::Ordering::Less);
        assert_eq!(cmp_id("c100", "c2"), std::cmp::Ordering::Greater);
        assert_eq!(cmp_id("retry", "retry-2"), std::cmp::Ordering::Less);
    }
}
