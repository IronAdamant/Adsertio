//! The Relatum adapter. A pointer `relatum:<store>#<note-id>` names one note
//! in a Relatum store (normally `.swarmnotes`): the file `<store>/<id>.md`.
//! It never runs Relatum and never writes into the store.
//!
//! A note is a state, not an event, like a Trammel constraint. It passes when
//! the note exists, its frontmatter is sound, and its `status` is `active`
//! (the default). A `superseded` note fails, and the detail names the note
//! that replaced it. A `draft` is inconclusive. The supersede chain is not
//! followed: a pointer judges the note it names.
//!
//! The frontmatter is read the way Relatum reads it: a leading byte order
//! mark is dropped, CRLF and CR become LF (Relatum writes CRLF on Windows),
//! the block opens with `---` on the first line and closes at the first
//! `\n---\n`, blank and `#` lines are skipped, and a repeated key keeps its
//! last value. Anything Relatum would not read as a note is `unreadable`.
//!
//! Relatum publishes each note whole (a hard link for a new note, a rename
//! for a supersede) and takes no lock, so a plain read sees the old note or
//! the new one. No lock is taken here either.

use crate::claim;
use crate::fsx;
use crate::read::{self, ReadError, MAX_EVIDENCE_BYTES};
use crate::verdict::{Evidence, State};
use std::path::Path;

const STATUSES: &[&str] = &["active", "draft", "superseded"];

pub fn evaluate(project: &Path, pointer: &str) -> Evidence {
    if let Err(text) = claim::valid_pointer(pointer) {
        return evidence(pointer, State::Unreadable, text);
    }
    let rest = pointer.split_once(':').map(|(_, rest)| rest).unwrap_or("");
    let (rel, id) = match rest.split_once('#') {
        Some((rel, id)) => (rel.trim_end_matches('/'), id),
        None => (rest.trim_end_matches('/'), ""),
    };
    if id.is_empty() {
        return evidence(
            pointer,
            State::Unreadable,
            "a note id is required after #, such as relatum:.swarmnotes#net/retry-decision",
        );
    }
    if !valid_id(id) {
        return evidence(
            pointer,
            State::Unreadable,
            format!(
                "\"{}\" is not a Relatum note id: segments match _?[a-z0-9][a-z0-9-]*, joined by /",
                fsx::clip(id, 60)
            ),
        );
    }
    match judge(project, rel, id) {
        Ok((state, detail)) => evidence(pointer, state, detail),
        Err(ReadError::Missing(text)) => evidence(pointer, State::Missing, text),
        Err(ReadError::Unreadable(text)) => evidence(pointer, State::Unreadable, text),
    }
}

fn evidence(pointer: &str, state: State, detail: impl Into<String>) -> Evidence {
    Evidence {
        pointer: pointer.to_string(),
        state,
        detail: detail.into(),
        at: None,
        commits: Vec::new(),
        trees: Vec::new(),
        malformed: false,
    }
}

/// A note id segment: `_?[a-z0-9][a-z0-9-]*` (Relatum `grammar::valid_segment`).
fn valid_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let start = usize::from(bytes.first() == Some(&b'_'));
    let alnum = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    bytes.get(start).is_some_and(alnum)
        && bytes[start + 1..]
            .iter()
            .all(|byte| alnum(byte) || *byte == b'-')
}

/// A note id: segments joined by `/` (Relatum `grammar::valid_id`).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.split('/').all(valid_segment)
}

/// The store, then the note in it, then its frontmatter.
fn judge(project: &Path, rel: &str, id: &str) -> Result<(State, String), ReadError> {
    let store = read::open_contained(project, rel).map_err(|error| match error {
        ReadError::Missing(_) => ReadError::Missing(format!(
            "{} is not there, so there is no Relatum store to read",
            fsx::clip(rel, 80)
        )),
        other => other,
    })?;
    let meta = std::fs::metadata(&store)
        .map_err(|e| ReadError::Unreadable(format!("cannot stat {rel}: {e}")))?;
    if !meta.is_dir() {
        return Err(ReadError::Unreadable(format!(
            "{} is not a folder; point at the Relatum store, such as relatum:.swarmnotes#<note-id>",
            fsx::clip(rel, 80)
        )));
    }
    let note_rel = format!("{rel}/{id}.md");
    let path = read::open_contained(project, &note_rel).map_err(|error| match error {
        ReadError::Missing(_) => ReadError::Missing(format!(
            "no note {id} in {} ({note_rel} is not there)",
            fsx::clip(rel, 80)
        )),
        other => other,
    })?;
    if !path.starts_with(&store) {
        return Err(ReadError::Unreadable(format!(
            "{note_rel} resolves outside the store {}",
            fsx::clip(rel, 80)
        )));
    }
    let bytes = read::read_bytes(&path, &note_rel, MAX_EVIDENCE_BYTES)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ReadError::Unreadable(format!("{note_rel} is not valid UTF-8")))?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let note = parse(&text)
        .map_err(|why| ReadError::Unreadable(format!("note {id} is damaged: {why}")))?;
    Ok(match note.status.as_str() {
        "active" => (
            State::Pass,
            format!("note {id} ({}) is active", fsx::clip(&note.kind, 40)),
        ),
        "draft" => (
            State::Inconclusive,
            format!("note {id} is a draft, not an agreed rule"),
        ),
        _ => {
            let by = note.superseded_by.unwrap_or_default();
            (
                State::Fail,
                format!(
                    "note {id} is superseded by {by}; point the claim at relatum:{rel}#{by} instead"
                ),
            )
        }
    })
}

struct Note {
    kind: String,
    status: String,
    superseded_by: Option<String>,
}

/// The frontmatter fields a verdict needs. An error says what is damaged.
fn parse(text: &str) -> Result<Note, String> {
    let Some(after) = text.strip_prefix("---\n") else {
        return Err("it does not start with a --- frontmatter line".into());
    };
    let Some(end) = after.find("\n---\n") else {
        return Err("the frontmatter is never closed with a --- line".into());
    };
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for line in after[..end].lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            format!(
                "frontmatter line \"{}\" is not key: value",
                fsx::clip(line.trim(), 60)
            )
        })?;
        let (key, value) = (key.trim(), value.trim());
        match fields.iter_mut().find(|(seen, _)| *seen == key) {
            Some(slot) => slot.1 = value,
            None => fields.push((key, value)),
        }
    }
    // Relatum reads `[a, b]` as a list, never as a kind, summary or status.
    let scalar = |key: &str| {
        fields
            .iter()
            .find(|(seen, _)| *seen == key)
            .map(|(_, value)| *value)
            .filter(|value| !(value.starts_with('[') && value.ends_with(']')))
    };
    let kind = scalar("kind")
        .filter(|value| !value.is_empty())
        .ok_or("kind is missing")?;
    scalar("summary")
        .filter(|value| !value.is_empty())
        .ok_or("summary is missing")?;
    let status = match fields.iter().find(|(seen, _)| *seen == "status") {
        None => "active",
        Some(_) => scalar("status").unwrap_or("[list]"),
    };
    if !STATUSES.contains(&status) {
        return Err(format!(
            "status \"{}\" is not active, draft or superseded",
            fsx::clip(status, 30)
        ));
    }
    let superseded_by = scalar("superseded_by").filter(|value| !value.is_empty());
    if status == "superseded" {
        match superseded_by {
            None => return Err("status is superseded but superseded_by is missing".into()),
            Some(by) if !valid_id(by) => {
                return Err(format!(
                    "superseded_by \"{}\" is not a note id",
                    fsx::clip(by, 60)
                ))
            }
            Some(_) => {}
        }
    } else {
        // Relatum's `resolve` (which `relatum get` uses) refuses a note that is
        // not superseded but names a replacement: it contradicts itself, so it
        // is not proof. A blank value or an empty list is no replacement.
        let raw = fields
            .iter()
            .find(|(seen, _)| *seen == "superseded_by")
            .map(|(_, value)| *value)
            .unwrap_or("");
        if !raw.is_empty() && raw.replace(' ', "") != "[]" {
            return Err(format!(
                "status is {status} but superseded_by is set; remove the superseded_by line, or finish the supersede with relatum supersede"
            ));
        }
    }
    Ok(Note {
        kind: kind.to_string(),
        status: status.to_string(),
        superseded_by: superseded_by.map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_the_relatum_grammar() {
        for good in ["a", "net/retry-decision", "_shared/style", "a/b/c-1", "0"] {
            assert!(valid_id(good), "{good}");
        }
        for bad in [
            "", "/a", "a/", "a//b", "_", "_-a", "-a", "A", "a b", "a.md", "..", "a/../b", "a_b",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn frontmatter_is_read_the_way_relatum_reads_it() {
        let ok = parse("---\nkind: decision\nsummary: s\n---\n\nbody\n").unwrap();
        assert_eq!(
            (ok.kind.as_str(), ok.status.as_str()),
            ("decision", "active")
        );
        let last = parse("---\nkind: a\nsummary: s\nstatus: draft\nstatus: active\n---\n").unwrap();
        assert_eq!(last.status, "active");
        // A blank or empty-list superseded_by names no replacement (Relatum agrees).
        for blank in [
            "superseded_by:\n",
            "superseded_by: []\n",
            "superseded_by: [ ]\n",
        ] {
            let n = parse(&format!("---\nkind: a\nsummary: s\n{blank}---\n")).unwrap();
            assert_eq!(n.status, "active", "{blank:?}");
        }
        let commented = parse("---\n# note\n  # indented\n\nkind: a\nsummary: s\n---\n").unwrap();
        assert_eq!(commented.kind, "a");
        for (text, why) in [
            ("kind: a\nsummary: s\n", "does not start"),
            ("---\nkind: a\nsummary: s\n", "never closed"),
            ("---\nkind: a\nsummary: s\n---", "never closed"),
            ("---\nkind a\nsummary: s\n---\n", "not key: value"),
            ("---\nsummary: s\n---\n", "kind is missing"),
            ("---\nkind:\nsummary: s\n---\n", "kind is missing"),
            ("---\nkind: [a]\nsummary: s\n---\n", "kind is missing"),
            ("---\nkind: a\n---\n", "summary is missing"),
            (
                "---\nkind: a\nsummary: s\nstatus: live\n---\n",
                "is not active",
            ),
            ("---\nkind: a\nsummary: s\nstatus:\n---\n", "is not active"),
            (
                "---\nkind: a\nsummary: s\nstatus: [active]\n---\n",
                "is not active",
            ),
            (
                "---\nkind: a\nsummary: s\nstatus: superseded\n---\n",
                "superseded_by is missing",
            ),
            (
                "---\nkind: a\nsummary: s\nstatus: superseded\nsuperseded_by: Bad Id\n---\n",
                "not a note id",
            ),
            (
                "---\nkind: a\nsummary: s\nsuperseded_by: b\n---\n",
                "status is active but superseded_by is set",
            ),
            (
                "---\nkind: a\nsummary: s\nstatus: draft\nsuperseded_by: b\n---\n",
                "status is draft but superseded_by is set",
            ),
            (
                "---\nkind: a\nsummary: s\nsuperseded_by: [b]\n---\n",
                "superseded_by is set",
            ),
        ] {
            let err = parse(text)
                .err()
                .unwrap_or_else(|| panic!("{text:?} parsed"));
            assert!(err.contains(why), "{text:?}: {err}");
        }
    }
}
