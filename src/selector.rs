//! The selector grammar, defined once. The readers (`trammel`, `chisel`,
//! `probatio`, `relatum`) parse a pointer's selector with these functions,
//! and `adsertio_claim` and `adsertio_attach` refuse, through [`check`], a
//! pointer whose shape can never be valid. So a pointer the writer accepts is
//! one the reader parses, and the other way round.
//!
//! Only the shape is checked here. Whether the file, the row or the note is
//! there, and whether a pass is fresh, is decided when the claim is viewed.

use crate::claim;
use crate::fsx;

/// A Trammel selector. Ids are the row `id`, digits only.
#[derive(Debug, Clone, PartialEq)]
pub enum Trammel {
    Step { plan: i64, step: i64 },
    Plan(i64),
    Constraint(i64),
    Failure(i64),
}

/// A Chisel selector.
#[derive(Debug, Clone, PartialEq)]
pub enum Chisel {
    Test(String),
    /// The path with backslashes made forward slashes.
    File(String),
}

/// A Probatio selector.
#[derive(Debug, Clone, PartialEq)]
pub enum Probatio {
    /// No selector, or `survivors`: the whole run.
    Run,
    Mutant(String),
    /// Counts entries; never a pass.
    Ledger,
}

/// Every accepted pointer shape, as `adsertio_guide` lists them.
pub const SHAPES: &[&str] = &[
    "trammel:.trammel/store#plan-<id>/step-<id>",
    "trammel:.trammel/store#plan-<id>",
    "trammel:.trammel/store#constraint-<id>",
    "trammel:.trammel/store#failure-<id>",
    "chisel:.chisel/store#test:<test_id>",
    "chisel:.chisel/store#file:<path>",
    "probatio:<out-dir>",
    "probatio:<out-dir>#survivors",
    "probatio:out/run#survivors",
    "probatio:<out-dir>#mutant:<id>",
    "probatio:<out-dir>#ledger",
    "relatum:.swarmnotes#<note-id>",
];

/// The expected shape for each tool, as a write error states it.
fn expected(tool: &str) -> &'static str {
    match tool {
        "trammel" => "trammel:<store>#plan-<id>/step-<id>, #plan-<id>, #constraint-<id> or #failure-<id> (ids are numbers), such as trammel:.trammel/store#plan-1/step-2",
        "chisel" => "chisel:<store>#test:<test_id> or chisel:<store>#file:<path>, such as chisel:.chisel/store#test:tests/a.rs:t1",
        "probatio" => "probatio:<out-dir>, or with #survivors, #mutant:<id> or #ledger",
        _ => "relatum:<store folder>#<note-id>",
    }
}

/// `(tool, path, selector)`. The selector is everything after the first `#`,
/// and `None` when there is no `#`.
pub fn split(pointer: &str) -> (&str, &str, Option<&str>) {
    let (tool, rest) = pointer.split_once(':').unwrap_or(("", pointer));
    match rest.split_once('#') {
        Some((path, selector)) => (tool, path, Some(selector)),
        None => (tool, rest, None),
    }
}

pub fn trammel(selector: &str) -> Result<Trammel, String> {
    if selector.is_empty() {
        return Err("a selector is required: plan-<id>/step-<id>, plan-<id>, constraint-<id>, or failure-<id>".into());
    }
    if let Some(rest) = selector.strip_prefix("plan-") {
        if let Some((plan, step)) = rest.split_once("/step-") {
            if !plan.is_empty() && !plan.contains('/') {
                return Ok(Trammel::Step {
                    plan: trammel_id(plan)?,
                    step: trammel_id(step)?,
                });
            }
        } else if !rest.is_empty() && !rest.contains('/') {
            return Ok(Trammel::Plan(trammel_id(rest)?));
        }
    } else if let Some(rest) = selector.strip_prefix("constraint-") {
        return Ok(Trammel::Constraint(trammel_id(rest)?));
    } else if let Some(rest) = selector.strip_prefix("failure-") {
        return Ok(Trammel::Failure(trammel_id(rest)?));
    }
    Err(format!(
        "unknown trammel selector \"{}\"",
        fsx::clip(selector, 60)
    ))
}

fn trammel_id(text: &str) -> Result<i64, String> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("\"{}\" is not an id", fsx::clip(text, 40)));
    }
    text.parse::<i64>()
        .map_err(|_| format!("\"{}\" is not an id", fsx::clip(text, 40)))
}

pub fn chisel(selector: &str) -> Result<Chisel, String> {
    if selector.is_empty() {
        return Err("a selector is required: test:<test_id> or file:<path>".into());
    }
    if let Some(id) = selector.strip_prefix("test:") {
        if id.is_empty() {
            return Err("test id is empty".into());
        }
        return Ok(Chisel::Test(id.to_string()));
    }
    if let Some(path) = selector.strip_prefix("file:") {
        if path.is_empty() || path.split(['/', '\\']).any(|part| part == "..") {
            return Err("file selector is empty or contains ..".into());
        }
        return Ok(Chisel::File(path.replace('\\', "/")));
    }
    Err(format!(
        "unknown chisel selector \"{}\"",
        fsx::clip(selector, 60)
    ))
}

pub fn probatio(selector: &str) -> Result<Probatio, String> {
    match selector {
        "" | "survivors" => return Ok(Probatio::Run),
        "ledger" => return Ok(Probatio::Ledger),
        _ => {}
    }
    if let Some(id) = selector.strip_prefix("mutant:") {
        if !mutant_id(id) {
            return Err(format!(
                "mutant id \"{}\" is not a single file name",
                fsx::clip(id, 60)
            ));
        }
        return Ok(Probatio::Mutant(id.to_string()));
    }
    Err(format!(
        "unknown probatio selector \"{}\"",
        fsx::clip(selector, 60)
    ))
}

/// A mutant id names one file in `results/`.
fn mutant_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
        && !id.contains("..")
}

/// The note id a Relatum selector names.
pub fn relatum(selector: &str) -> Result<&str, String> {
    if selector.is_empty() {
        return Err(
            "a note id is required after #, such as relatum:.swarmnotes#net/retry-decision".into(),
        );
    }
    if !note_id(selector) {
        return Err(format!(
            "\"{}\" is not a Relatum note id: segments match _?[a-z0-9][a-z0-9-]*, joined by /",
            fsx::clip(selector, 60)
        ));
    }
    Ok(selector)
}

/// A note id segment: `_?[a-z0-9][a-z0-9-]*` (Relatum `grammar::valid_segment`).
fn note_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let start = usize::from(bytes.first() == Some(&b'_'));
    let alnum = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    bytes.get(start).is_some_and(alnum)
        && bytes[start + 1..]
            .iter()
            .all(|byte| alnum(byte) || *byte == b'-')
}

/// A note id: segments joined by `/` (Relatum `grammar::valid_id`).
pub fn note_id(id: &str) -> bool {
    !id.is_empty() && id.split('/').all(note_segment)
}

/// The write-time check: the path rules of [`claim::valid_pointer`], then the
/// selector's shape for that tool. A Relatum path must name the store folder,
/// so one ending in `.md` is refused. The error is one line: the pointer,
/// what is wrong, the expected shape, and a suggestion when one can be
/// inferred.
pub fn check(pointer: &str) -> Result<(), String> {
    claim::valid_pointer(pointer)?;
    let (tool, path, selector) = split(pointer);
    let sel = selector.unwrap_or("");
    let note_file = tool == "relatum" && path.trim_end_matches('/').ends_with(".md");
    let refused = match tool {
        "trammel" => trammel(sel).err(),
        "chisel" => chisel(sel).err(),
        "probatio" => probatio(sel).err(),
        "relatum" if note_file => Some("the path names a note file, not the store folder".into()),
        "relatum" => relatum(sel).err(),
        _ => None,
    };
    let Some(why) = refused else {
        return Ok(());
    };
    // The readers' text for a missing selector lists the shapes, which the
    // expected shape below lists again.
    let why = match tool {
        "relatum" if sel.is_empty() && !note_file => "no #<note-id>".into(),
        "trammel" | "chisel" if sel.is_empty() => "no selector".into(),
        _ => why,
    };
    let mut text = format!(
        "evidence \"{}\": {}. Expected {}",
        fsx::clip(pointer, 80),
        why.trim_end_matches('.'),
        expected(tool)
    );
    if let Some(better) = suggest(tool, path, selector) {
        text.push_str(&format!("; did you mean {better}?"));
    }
    Err(text)
}

/// A corrected pointer, when the mistake is a common one and the fix is
/// plain from the text alone. It reads no files.
///
/// - Chisel `#<x>` with no `test:` or `file:`: `#file:<x>` when `<x>` has a
///   `/`, else `#test:<x>`.
/// - Relatum `<x>` with no `#`, `<x>` a note id: `.swarmnotes#<x>`.
/// - Relatum `<...>/.swarmnotes/<id>` or `<...>/.swarmnotes/<id>.md`, with or
///   without a selector after the `.md`: split at the `.swarmnotes` folder.
fn suggest(tool: &str, path: &str, selector: Option<&str>) -> Option<String> {
    let fixed = match tool {
        "chisel" => {
            let sel = selector.filter(|s| !s.is_empty() && !s.contains(':'))?;
            let kind = if sel.contains('/') { "file" } else { "test" };
            format!("chisel:{path}#{kind}:{sel}")
        }
        "relatum" => {
            let path = path.trim_end_matches('/');
            let (stem, file) = match path.strip_suffix(".md") {
                Some(stem) => (stem, true),
                None => (path, false),
            };
            if selector.is_some() && !file {
                return None;
            }
            let parts: Vec<&str> = stem.split('/').collect();
            let (store, id) = match parts.iter().position(|part| *part == ".swarmnotes") {
                Some(at) => (parts[..=at].join("/"), parts[at + 1..].join("/")),
                None if !file => (".swarmnotes".to_string(), stem.to_string()),
                None => return None,
            };
            format!("relatum:{store}#{id}")
        }
        _ => return None,
    };
    check(&fixed).is_ok().then_some(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_ids_follow_the_relatum_grammar() {
        for good in ["a", "net/retry-decision", "_shared/style", "a/b/c-1", "0"] {
            assert!(note_id(good), "{good}");
        }
        for bad in [
            "", "/a", "a/", "a//b", "_", "_-a", "-a", "A", "a b", "a.md", "..", "a/../b", "a_b",
        ] {
            assert!(!note_id(bad), "{bad}");
        }
    }

    #[test]
    fn split_takes_the_first_hash() {
        assert_eq!(
            split("chisel:.chisel/store#test:a#b"),
            ("chisel", ".chisel/store", Some("test:a#b"))
        );
        assert_eq!(split("probatio:out"), ("probatio", "out", None));
        assert_eq!(split("trammel:s#"), ("trammel", "s", Some("")));
    }

    #[test]
    fn every_guide_shape_is_accepted() {
        for shape in SHAPES {
            let real = shape
                .replace("<id>", "1")
                .replace("<test_id>", "tests/a.rs:t1")
                .replace("<path>", "src/a.rs")
                .replace("<out-dir>", ".probatio/run")
                .replace("<note-id>", "net/retry");
            assert!(check(&real).is_ok(), "{real}: {:?}", check(&real));
        }
    }
}
