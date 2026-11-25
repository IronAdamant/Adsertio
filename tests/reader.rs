//! The shared reader: journal wins over a stale snapshot, damage is not an
//! empty store, compact tables expand, SQLite is refused, and a path that
//! leaves the project is refused. Nothing in the other tools' folders is
//! written or deleted.
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("adsertio-read-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}

const STORE: &str = r#"{"schema_version":1,"plans":[{"id":1,"goal":"old"}],"steps":[],"constraints":[],"failures":[]}"#;
const JOURNAL: &str = r#"{"schema_version":1,"plans":[{"id":2,"goal":"from journal"}],"steps":[],"constraints":[],"failures":[]}"#;

#[test]
fn a_journal_line_replaces_the_snapshot() {
    let project = scratch("journal");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), STORE).unwrap();
    fs::write(dir.join("journal"), format!("\n{JOURNAL}\n")).unwrap();
    fs::write(dir.join("store.tmp"), b"do not delete").unwrap();
    let before = names(&dir);
    let snap = adsertio::read::read_trammel(&project, ".trammel/store").unwrap();
    assert!(snap.journal_applied);
    let goal = snap.doc.get("plans").unwrap().as_array().unwrap()[0]
        .get("goal")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(goal, "from journal");
    assert_eq!(fs::read(dir.join("store.tmp")).unwrap(), b"do not delete");
    assert_eq!(
        names(&dir),
        before,
        "the reader must not write a lock or delete the journal"
    );
    assert!(!dir.join("lock").exists());
}

#[test]
fn two_journal_lines_keep_the_last_document() {
    let project = scratch("journal-two");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), STORE).unwrap();
    fs::write(dir.join("journal"), format!("{STORE}\n{JOURNAL}\n")).unwrap();
    let snap = adsertio::read::read_trammel(&project, ".trammel/store").unwrap();
    assert_eq!(
        snap.doc.get("plans").unwrap().as_array().unwrap()[0]
            .get("id")
            .unwrap()
            .as_i64(),
        Some(2)
    );
}

#[test]
fn a_corrupt_store_is_unreadable_not_an_empty_plan_list() {
    let project = scratch("corrupt");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), b"{\"schema_version\":1,").unwrap();
    fs::write(dir.join("journal"), JOURNAL).unwrap();
    let err = adsertio::read::read_trammel(&project, ".trammel/store").unwrap_err();
    let text = err.text();
    assert!(!err.is_missing(), "{text}");
    assert!(text.contains("not valid JSON"), "{text}");
    assert!(!text.contains("plans"), "{text}");
}

#[test]
fn a_store_without_schema_version_is_unreadable() {
    let project = scratch("noschema");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), b"{\"plans\":[]}").unwrap();
    let text = adsertio::read::read_trammel(&project, ".trammel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("schema_version"), "{text}");
    assert!(text.contains("not treated as empty"), "{text}");
}

#[test]
fn a_bad_journal_line_is_unreadable() {
    let project = scratch("bad-journal");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), STORE).unwrap();
    fs::write(dir.join("journal"), format!("{JOURNAL}\nnot-json\n")).unwrap();
    let text = adsertio::read::read_trammel(&project, ".trammel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("journal line"), "{text}");
    fs::write(dir.join("journal"), "{\"plans\":[]}\n").unwrap();
    let text = adsertio::read::read_trammel(&project, ".trammel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("schema_version"), "{text}");
}

#[test]
fn a_lock_that_is_a_folder_is_unreadable_and_deep_json_does_not_crash() {
    let project = scratch("lock-dir");
    let dir = project.join(".trammel");
    fs::create_dir_all(dir.join("lock")).unwrap();
    fs::write(dir.join("store"), STORE).unwrap();
    let text = adsertio::read::read_trammel(&project, ".trammel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("lock"), "{text}");

    let project = scratch("deep");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    let deep = format!(
        "{{\"schema_version\":1,\"plans\":{},\"steps\":[],\"constraints\":[],\"failures\":[]}}",
        "[".repeat(80) + &"]".repeat(80)
    );
    fs::write(dir.join("store"), deep).unwrap();
    assert!(adsertio::read::read_trammel(&project, ".trammel/store").is_err());
}

#[test]
fn an_oversized_snapshot_is_not_read() {
    let project = scratch("huge");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    let file = fs::File::create(dir.join("store")).unwrap();
    file.set_len(adsertio::read::MAX_EVIDENCE_BYTES + 1)
        .unwrap();
    let text = adsertio::read::read_trammel(&project, ".trammel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("larger than"), "{text}");
}

#[test]
fn wrong_magic_is_unreadable_and_a_compact_table_expands() {
    let project = scratch("magic");
    let dir = project.join(".chisel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), r#"{"magic":"nope","version":1}"#).unwrap();
    fs::write(dir.join("store.tmp"), b"keep").unwrap();
    let before = names(&dir);
    let text = adsertio::read::read_chisel(&project, ".chisel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("magic"), "{text}");
    assert_eq!(fs::read(dir.join("store.tmp")).unwrap(), b"keep");
    assert_eq!(before, names(&dir));
    assert!(!dir.join("chisel.lock").exists());

    let compact = r#"{
        "magic":"chisel-store",
        "version":1,
        "test_edges":{
            "table":1,
            "cols":["test_id","code_id","edge_type","weight"],
            "dict":[true,true,true,false],
            "strings":["tests/a.rs:t1","src/a.rs:f:function","call"],
            "rows":[[0,1,2,1.0]]
        },
        "blame":[]
    }"#;
    fs::write(dir.join("store"), compact).unwrap();
    let snap = adsertio::read::read_chisel(&project, ".chisel/store").unwrap();
    let row = &snap.doc.get("test_edges").unwrap().as_array().unwrap()[0];
    assert_eq!(row.get("test_id").unwrap().as_str(), Some("tests/a.rs:t1"));
    assert_eq!(
        row.get("code_id").unwrap().as_str(),
        Some("src/a.rs:f:function")
    );
    assert_eq!(row.get("weight").unwrap().as_f64(), Some(1.0));
}

#[test]
fn a_broken_compact_table_is_unreadable() {
    let project = scratch("compact-bad");
    let dir = project.join(".chisel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("store"),
        r#"{"magic":"chisel-store","version":1,"test_edges":{"table":1}}"#,
    )
    .unwrap();
    let text = adsertio::read::read_chisel(&project, ".chisel/store")
        .unwrap_err()
        .text()
        .to_string();
    assert!(text.contains("compact table"), "{text}");

    fs::write(
        dir.join("store"),
        r#"{"magic":"chisel-store","version":1,"test_edges":{"table":2,"cols":[],"dict":[],"strings":[],"rows":[]}}"#,
    )
    .unwrap();
    assert!(adsertio::read::read_chisel(&project, ".chisel/store")
        .unwrap_err()
        .text()
        .contains("not 1"));
}

#[test]
fn sqlite_is_unreadable_and_names_migrate() {
    let project = scratch("sqlite");
    fs::create_dir_all(project.join(".trammel")).unwrap();
    fs::create_dir_all(project.join(".chisel")).unwrap();
    let header = b"SQLite format 3\x00rest of header";
    fs::write(project.join(".trammel/store"), header).unwrap();
    fs::write(project.join(".chisel/store"), header).unwrap();
    let trammel = adsertio::read::read_trammel(&project, ".trammel/store").unwrap_err();
    assert!(
        trammel.text().contains("migrate") && trammel.text().contains("trammel"),
        "{}",
        trammel.text()
    );
    let chisel = adsertio::read::read_chisel(&project, ".chisel/store").unwrap_err();
    assert!(
        chisel.text().contains("migrate") && chisel.text().contains("chisel"),
        "{}",
        chisel.text()
    );
}

#[test]
fn paths_outside_the_project_fail() {
    let project = scratch("outside");
    fs::create_dir_all(project.join(".trammel")).unwrap();
    let err = adsertio::read::read_trammel(&project, "../secret").unwrap_err();
    assert!(!err.is_missing(), "{}", err.text());
    assert!(
        err.text().contains("project") || err.text().contains("relative"),
        "{}",
        err.text()
    );
    let missing = adsertio::read::read_chisel(&project, ".chisel/store").unwrap_err();
    assert!(missing.is_missing(), "{}", missing.text());
}

#[cfg(unix)]
#[test]
fn a_journal_symlink_outside_the_project_is_not_read() {
    let project = scratch("symlink-journal");
    let outside = scratch("symlink-journal-out");
    let dir = project.join(".trammel");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("store"), STORE).unwrap();
    fs::write(outside.join("journal"), JOURNAL).unwrap();
    std::os::unix::fs::symlink(outside.join("journal"), dir.join("journal")).unwrap();
    let err = adsertio::read::read_trammel(&project, ".trammel/store").unwrap_err();
    assert!(
        err.text().contains("outside"),
        "followed a journal symlink: {}",
        err.text()
    );
}
