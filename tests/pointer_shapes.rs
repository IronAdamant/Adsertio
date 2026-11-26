//! 1.5.0: a pointer whose selector has a shape that can never be valid is
//! refused at `adsertio_claim` and `adsertio_attach`, through the CLI and MCP,
//! single and batch, and nothing is written. The writer and the readers use
//! one grammar (`adsertio::selector`), so a shape the writer accepts the
//! reader parses, and the other way round. A pointer already stored is still
//! read as before.
mod common;
use adsertio::selector;
use adsertio::verdict::{read_evidence, State};
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const TRAMMEL: &str = include_str!("fixtures/trammel/store.json");
const CHISEL: &str = include_str!("fixtures/chisel/store.json");

fn copy_tree(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dest.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// A project with a store of each tool: Trammel and Chisel snapshots, a
/// Probatio run in `out`, and a Relatum store with one note.
fn stores(name: &str) -> PathBuf {
    let dir = project(name);
    init(&dir);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), TRAMMEL).unwrap();
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(dir.join(".chisel/store"), CHISEL).unwrap();
    let clean = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/probatio/clean");
    copy_tree(&clean, &dir.join("out"));
    let note = dir.join(".swarmnotes/net/retry.md");
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    fs::write(
        &note,
        "---\nkind: decision\nsummary: A rule.\n---\n\nBody.\n",
    )
    .unwrap();
    dir
}

fn pointer(base: &str, selector: Option<&str>) -> String {
    match selector {
        Some(sel) => format!("{base}#{sel}"),
        None => base.to_string(),
    }
}

/// `(pointer base, selector, accepted)` for every tool.
const TABLE: &[(&str, Option<&str>, bool)] = &[
    ("trammel:.trammel/store", Some("plan-1/step-1"), true),
    ("trammel:.trammel/store", Some("plan-1/step-99"), true),
    ("trammel:.trammel/store", Some("plan-1"), true),
    ("trammel:.trammel/store", Some("plan-01"), true),
    ("trammel:.trammel/store", Some("constraint-1"), true),
    ("trammel:.trammel/store", Some("failure-7"), true),
    ("trammel:.trammel/store", None, false),
    ("trammel:.trammel/store", Some(""), false),
    ("trammel:.trammel/store", Some("step-2"), false),
    ("trammel:.trammel/store", Some("plan-1/step-"), false),
    ("trammel:.trammel/store", Some("plan-/step-1"), false),
    ("trammel:.trammel/store", Some("plan-x"), false),
    ("trammel:.trammel/store", Some("plan-1/"), false),
    ("trammel:.trammel/store", Some("plan-1/step-2/x"), false),
    ("trammel:.trammel/store", Some("constraint-"), false),
    ("trammel:.trammel/store", Some("failure--1"), false),
    (
        "trammel:.trammel/store",
        Some("plan-99999999999999999999"),
        false,
    ),
    ("trammel:.trammel/store", Some("1"), false),
    ("chisel:.chisel/store", Some("test:tests/a.rs:t1"), true),
    ("chisel:.chisel/store", Some("test:nope"), true),
    ("chisel:.chisel/store", Some("test:a#b"), true),
    ("chisel:.chisel/store", Some("file:src/a.rs"), true),
    ("chisel:.chisel/store", Some(r"file:src\a.rs"), true),
    ("chisel:.chisel/store", None, false),
    ("chisel:.chisel/store", Some(""), false),
    ("chisel:.chisel/store", Some("t1"), false),
    ("chisel:.chisel/store", Some("test:"), false),
    ("chisel:.chisel/store", Some("file:"), false),
    ("chisel:.chisel/store", Some("file:../x"), false),
    ("chisel:.chisel/store", Some("file:a/../b"), false),
    ("chisel:.chisel/store", Some("TEST:x"), false),
    ("probatio:out", None, true),
    ("probatio:out", Some(""), true),
    ("probatio:out", Some("survivors"), true),
    ("probatio:out", Some("ledger"), true),
    ("probatio:out", Some("mutant:m1"), true),
    ("probatio:out", Some("mutant:nope"), true),
    ("probatio:out", Some("mutant:"), false),
    ("probatio:out", Some("mutant:a/b"), false),
    ("probatio:out", Some("mutant:.."), false),
    ("probatio:out", Some("mutant:a..b"), false),
    ("probatio:out", Some("survivor"), false),
    ("probatio:out", Some("Ledger"), false),
    ("relatum:.swarmnotes", Some("net/retry"), true),
    ("relatum:.swarmnotes", Some("net/nope"), true),
    ("relatum:.swarmnotes", Some("_shared/style"), true),
    ("relatum:.swarmnotes/", Some("net/retry"), true),
    ("relatum:.swarmnotes", None, false),
    ("relatum:.swarmnotes", Some(""), false),
    ("relatum:.swarmnotes", Some("Net"), false),
    ("relatum:.swarmnotes", Some("net/"), false),
    ("relatum:.swarmnotes", Some("net/retry.md"), false),
    ("relatum:.swarmnotes", Some("a//b"), false),
    ("relatum:.swarmnotes", Some("-a"), false),
];

/// What the shared grammar says about a selector, by tool.
fn grammar(tool: &str, sel: &str) -> Result<(), String> {
    match tool {
        "trammel" => selector::trammel(sel).map(drop),
        "chisel" => selector::chisel(sel).map(drop),
        "probatio" => selector::probatio(sel).map(drop),
        _ => selector::relatum(sel).map(drop),
    }
}

#[test]
fn the_writer_and_the_readers_agree_on_every_selector() {
    let dir = stores("shapes-agree");
    for (base, sel, accepted) in TABLE {
        let p = pointer(base, *sel);
        let (tool, _, _) = selector::split(&p);
        let parsed = grammar(tool, sel.unwrap_or(""));
        assert_eq!(parsed.is_ok(), *accepted, "grammar: {p}: {parsed:?}");
        let written = selector::check(&p);
        assert_eq!(written.is_ok(), *accepted, "writer: {p}: {written:?}");
        let read = read_evidence(&dir, &p);
        if *accepted {
            assert_ne!(
                read.state,
                State::Unreadable,
                "reader: {p}: {}",
                read.detail
            );
        } else {
            assert_eq!(
                read.state,
                State::Unreadable,
                "reader: {p}: {}",
                read.detail
            );
            assert_eq!(read.detail, parsed.unwrap_err(), "reader: {p}");
        }
    }
}

#[test]
fn a_relatum_path_must_name_the_store_folder() {
    let dir = stores("shapes-relatum-path");
    for p in [
        "relatum:.swarmnotes/net/retry.md",
        "relatum:.swarmnotes/net/retry.md#net/retry",
        "relatum:.swarmnotes/net/retry.md/#net/retry",
        "relatum:notes/a.md#a",
    ] {
        let err = selector::check(p).unwrap_err();
        assert!(
            err.contains("names a note file, not the store folder"),
            "{p}: {err}"
        );
        // Stored, it reads as before: the file is not a folder, or not there.
        let read = read_evidence(&dir, p);
        assert!(
            matches!(read.state, State::Unreadable | State::Missing),
            "{p}: {:?} {}",
            read.state,
            read.detail
        );
    }
    assert!(selector::check("relatum:.swarmnotes/#net/retry").is_ok());
    assert!(selector::check("relatum:notes.md.d#net/retry").is_ok());
}

fn cli(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn listing(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(d: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(&p, out);
            }
            let bytes = if p.is_file() {
                fs::read(&p).unwrap()
            } else {
                Vec::new()
            };
            out.push((p, bytes));
        }
    }
    let mut out = Vec::new();
    walk(&dir.join(".adsertio"), &mut out);
    out
}

/// The five pointers agents wrote first in real sessions, and the one-line
/// error each now gets.
const OBSERVED: &[(&str, &str)] = &[
    (
        "relatum:_shared/no-deps",
        "evidence \"relatum:_shared/no-deps\": no #<note-id>. Expected relatum:<store folder>#<note-id>; did you mean relatum:.swarmnotes#_shared/no-deps?",
    ),
    (
        "relatum:.swarmnotes/_shared/no-deps.md",
        "evidence \"relatum:.swarmnotes/_shared/no-deps.md\": the path names a note file, not the store folder. Expected relatum:<store folder>#<note-id>; did you mean relatum:.swarmnotes#_shared/no-deps?",
    ),
    (
        "trammel:.trammel/store",
        "evidence \"trammel:.trammel/store\": no selector. Expected trammel:<store>#plan-<id>/step-<id>, #plan-<id>, #constraint-<id> or #failure-<id> (ids are numbers), such as trammel:.trammel/store#plan-1/step-2",
    ),
    (
        "chisel:.chisel/store#t1",
        "evidence \"chisel:.chisel/store#t1\": unknown chisel selector \"t1\". Expected chisel:<store>#test:<test_id> or chisel:<store>#file:<path>, such as chisel:.chisel/store#test:tests/a.rs:t1; did you mean chisel:.chisel/store#test:t1?",
    ),
    (
        "probatio:.probatio/run#mutant:",
        "evidence \"probatio:.probatio/run#mutant:\": mutant id \"\" is not a single file name. Expected probatio:<out-dir>, or with #survivors, #mutant:<id> or #ledger",
    ),
];

#[test]
fn the_observed_pointers_are_refused_with_the_fix() {
    let dir = stores("shapes-observed");
    for (p, want) in OBSERVED {
        assert_eq!(selector::check(p).unwrap_err(), *want);
        let before = listing(&dir);
        let (ok, text) = cli(&dir, &["claim", "--summary", "s", "--evidence", p]);
        assert!(!ok, "{p}: {text}");
        assert_eq!(text.trim_end(), format!("error: {want}"), "{p}");
        assert_eq!(text.lines().count(), 1, "{text}");
        assert_eq!(listing(&dir), before, "{p}");
    }
    // `<store>/<id>` with no `.md` and no `#` is split at the store too.
    let err = selector::check("relatum:.swarmnotes/_shared/no-deps").unwrap_err();
    assert!(
        err.ends_with("; did you mean relatum:.swarmnotes#_shared/no-deps?"),
        "{err}"
    );
    // No suggestion when the fix is not plain from the text.
    for p in [
        "relatum:notes/a.md#a",
        "relatum:.swarmnotes",
        "relatum:Net/Retry",
        "relatum:.swarmnotes#Net",
        "trammel:.trammel/store#step-2",
        "chisel:.chisel/store",
        "chisel:.chisel/store#test:",
        "probatio:out#survivor",
    ] {
        let err = selector::check(p).unwrap_err();
        assert!(!err.contains("did you mean"), "{p}: {err}");
        assert!(err.contains("Expected "), "{p}: {err}");
    }
    let chisel_file = selector::check("chisel:.chisel/store#src/a.rs").unwrap_err();
    assert!(
        chisel_file.ends_with("did you mean chisel:.chisel/store#file:src/a.rs?"),
        "{chisel_file}"
    );
}

/// One refused pointer per tool.
const REFUSED: &[&str] = &[
    "trammel:.trammel/store",
    "trammel:.trammel/store#step-2",
    "chisel:.chisel/store",
    "chisel:.chisel/store#t1",
    "probatio:out#mutant:",
    "probatio:out#survivor",
    "relatum:_shared/no-deps",
    "relatum:.swarmnotes/net/retry.md",
    "relatum:.swarmnotes#Net",
];

const GOOD: &str = "trammel:.trammel/store#plan-1/step-1";

#[test]
fn claim_refuses_a_bad_shape_through_mcp_and_cli_and_writes_nothing() {
    let dir = stores("shapes-claim");
    for bad in REFUSED {
        let before = listing(&dir);
        let single = format!(r#"{{"summary":"s","id":"one","evidence":["{GOOD}","{bad}"]}}"#);
        let batch = format!(
            r#"{{"claims":[{{"summary":"a","id":"first","evidence":["{GOOD}"]}},{{"summary":"b","id":"second","evidence":["{bad}"]}}]}}"#
        );
        let mut input = call("adsertio_claim", &single, 1);
        input += &call("adsertio_claim", &batch, 2);
        let r = talk(&dir, input.as_bytes());
        assert!(
            is_error(&r[0]) && r[0].contains("Expected "),
            "{bad}: {}",
            r[0]
        );
        assert!(
            is_error(&r[1])
                && r[1].contains("claims item 2: evidence")
                && r[1].contains("Nothing was written."),
            "{bad}: {}",
            r[1]
        );
        assert_eq!(listing(&dir), before, "{bad}");

        let (ok, text) = cli(
            &dir,
            &[
                "claim",
                "--summary",
                "s",
                "--evidence",
                GOOD,
                "--evidence",
                bad,
            ],
        );
        assert!(!ok && text.contains("Expected "), "{bad}: {text}");
        let items = format!(
            r#"[{{"summary":"a","id":"first","evidence":["{GOOD}"]}},{{"summary":"b","evidence":["{bad}"]}}]"#
        );
        let (ok, text) = cli(&dir, &["claim", "--claims", &items]);
        assert!(
            !ok && text.contains("claims item 2: evidence")
                && text.contains("Nothing was written."),
            "{bad}: {text}"
        );
        assert_eq!(listing(&dir), before, "{bad}");
    }
    let suggest = format!(
        r#"{{"claims":[{{"summary":"a","evidence":["{GOOD}"]}},{{"summary":"b","evidence":["relatum:_shared/no-deps"]}}]}}"#
    );
    let r = talk(&dir, call("adsertio_claim", &suggest, 1).as_bytes());
    assert!(
        r[0].contains("did you mean relatum:.swarmnotes#_shared/no-deps? Nothing was written."),
        "{}",
        r[0]
    );
}

#[test]
fn attach_refuses_a_bad_shape_through_mcp_and_cli_and_writes_nothing() {
    let dir = stores("shapes-attach");
    for id in ["a", "b"] {
        let (ok, text) = cli(&dir, &["claim", "--summary", id, "--id", id]);
        assert!(ok, "{text}");
    }
    for bad in REFUSED {
        let before = listing(&dir);
        let single = format!(r#"{{"id":"a","evidence":["{GOOD}","{bad}"]}}"#);
        let batch = format!(
            r#"{{"items":[{{"id":"a","evidence":["{GOOD}"]}},{{"id":"b","evidence":["{bad}"]}}]}}"#
        );
        let mut input = call("adsertio_attach", &single, 1);
        input += &call("adsertio_attach", &batch, 2);
        let r = talk(&dir, input.as_bytes());
        assert!(
            is_error(&r[0]) && r[0].contains("Expected "),
            "{bad}: {}",
            r[0]
        );
        assert!(
            is_error(&r[1])
                && r[1].contains("attach item 2: evidence")
                && r[1].contains("Nothing was written."),
            "{bad}: {}",
            r[1]
        );
        assert_eq!(listing(&dir), before, "{bad}");

        let (ok, text) = cli(
            &dir,
            &["attach", "--id", "a", "--evidence", GOOD, "--evidence", bad],
        );
        assert!(!ok && text.contains("Expected "), "{bad}: {text}");
        let items =
            format!(r#"[{{"id":"a","evidence":["{GOOD}"]}},{{"id":"b","evidence":["{bad}"]}}]"#);
        let (ok, text) = cli(&dir, &["attach", "--items", &items]);
        assert!(
            !ok && text.contains("attach item 2: evidence")
                && text.contains("Nothing was written."),
            "{bad}: {text}"
        );
        assert_eq!(listing(&dir), before, "{bad}");
    }
}

#[test]
fn every_good_shape_is_written_by_claim_and_attach() {
    let dir = stores("shapes-good");
    let good: Vec<String> = TABLE
        .iter()
        .filter(|(_, _, ok)| *ok)
        .map(|(base, sel, _)| pointer(base, *sel))
        .collect();
    let list = good
        .iter()
        .map(|p| format!("\"{}\"", p.replace('\\', "\\\\")))
        .collect::<Vec<_>>()
        .join(",");
    let mut input = call(
        "adsertio_claim",
        &format!(r#"{{"summary":"s","id":"mcp","evidence":[{list}]}}"#),
        1,
    );
    input += &call(
        "adsertio_claim",
        &format!(r#"{{"claims":[{{"summary":"t","id":"mcp-batch","evidence":[{list}]}}]}}"#),
        2,
    );
    input += &call("adsertio_claim", r#"{"summary":"u","id":"mcp-attach"}"#, 3);
    input += &call(
        "adsertio_attach",
        &format!(r#"{{"items":[{{"id":"mcp-attach","evidence":[{list}]}}]}}"#),
        4,
    );
    let r = talk(&dir, input.as_bytes());
    for reply in &r {
        assert!(is_ok(reply), "{reply}");
    }
    let mut args = vec!["claim", "--summary", "v", "--id", "cli"];
    for p in &good {
        args.push("--evidence");
        args.push(p);
    }
    let (ok, text) = cli(&dir, &args);
    assert!(ok, "{text}");
    let (ok, text) = cli(&dir, &["claim", "--summary", "w", "--id", "cli-attach"]);
    assert!(ok, "{text}");
    let mut args = vec!["attach", "--id", "cli-attach"];
    for p in &good {
        args.push("--evidence");
        args.push(p);
    }
    let (ok, text) = cli(&dir, &args);
    assert!(ok, "{text}");
    for id in ["mcp", "mcp-batch", "mcp-attach", "cli", "cli-attach"] {
        let text = fs::read_to_string(dir.join(format!(".adsertio/claims/{id}.md"))).unwrap();
        for p in &good {
            assert!(text.contains(p.as_str()), "{id}: {p}");
        }
    }
}

#[test]
fn a_stored_bad_pointer_still_views_as_unreadable() {
    let dir = stores("shapes-stored");
    let bad: Vec<String> = TABLE
        .iter()
        .filter(|(_, _, ok)| !*ok)
        .map(|(base, sel, _)| pointer(base, *sel))
        .collect();
    let refs: Vec<&str> = bad.iter().map(String::as_str).collect();
    write_claim(&dir, "old", &refs);
    let (ok, text) = cli(&dir, &["view", "--id", "old"]);
    assert!(ok, "{text}");
    let doc = adsertio::json::parse(&text).unwrap();
    let claim = &doc.get("claims").unwrap().as_array().unwrap()[0];
    assert_eq!(
        claim.get("verdict").unwrap().as_str(),
        Some("unverified"),
        "{text}"
    );
    let states = claim.get("evidence").unwrap().as_array().unwrap();
    assert_eq!(states.len(), bad.len(), "{text}");
    for e in states {
        assert_eq!(
            e.get("state").unwrap().as_str(),
            Some("unreadable"),
            "{text}"
        );
    }
    // The claim is not damaged: check reads it, and attach still adds to it.
    let (ok, text) = cli(&dir, &["check"]);
    assert!(ok, "{text}");
    let (ok, text) = cli(&dir, &["attach", "--id", "old", "--evidence", GOOD]);
    assert!(ok, "{text}");
}
