//! Regression tests for the fourth review (1.2.1): the Chisel root store
//! however it is spelt, Probatio outcomes and unfinished runs, a one-part
//! ADSERTIO_DIR, cursors from a hand-written time, the Trammel journal beside
//! a symlinked store, and a capped Chisel detail.
mod common;
use adsertio::verdict::State;
use common::*;
use std::fs;
use std::path::Path;
use std::process::Command;

fn sharded(name: &str) -> std::path::PathBuf {
    let dir = project(name);
    fs::create_dir_all(dir.join(".chisel/shards/net")).unwrap();
    fs::write(
        dir.join(".chisel/store"),
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"tests/a.rs:t2","passed":true,"recorded_at":4102444700}],"test_edges":[]}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".chisel/shards/net/store"),
        include_str!("fixtures/chisel/sharded-net.json"),
    )
    .unwrap();
    dir
}

#[test]
fn every_spelling_of_the_chisel_root_store_reads_the_shards() {
    let dir = sharded("r4-chisel-alias");
    for path in [
        ".chisel/store",
        "./.chisel/store",
        ".chisel/./store",
        ".chisel//store",
    ] {
        // The root's own pass stands: Chisel records results only in the
        // root store, so the shard's older-style failure row is not read.
        let e = adsertio::chisel::evaluate(&dir, &format!("chisel:{path}#test:tests/a.rs:t2"));
        assert_eq!(e.state, State::Pass, "{path}: {}", e.detail);
        // The shard's edge is read: only it links t2 to src/a.rs.
        let e = adsertio::chisel::evaluate(&dir, &format!("chisel:{path}#file:src/a.rs"));
        assert_eq!(e.state, State::Pass, "{path}: {}", e.detail);
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_chisel_folder_still_reads_the_shards() {
    let dir = sharded("r4-chisel-link");
    std::os::unix::fs::symlink(dir.join(".chisel"), dir.join("chisel-link")).unwrap();
    let e = adsertio::chisel::evaluate(&dir, "chisel:chisel-link/store#file:src/a.rs");
    assert_eq!(e.state, State::Pass, "{}", e.detail);
}

#[test]
fn a_long_chisel_wait_list_is_capped() {
    let dir = project("r4-chisel-cap");
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    let mut strings: Vec<String> = (0..15).map(|i| format!("\"tests/b.rs:t{i}\"")).collect();
    strings.push("\"src/big.rs:f:function\"".into());
    strings.push("\"call\"".into());
    let rows: Vec<String> = (0..15).map(|i| format!("[{i},15,16,1]")).collect();
    fs::write(
        dir.join(".chisel/store"),
        format!(
            r#"{{"magic":"chisel-store","version":1,"test_results":[],"test_edges":{{"table":1,"cols":["test_id","code_id","edge_type","weight"],"dict":[true,true,true,false],"strings":[{}],"rows":[{}]}}}}"#,
            strings.join(","),
            rows.join(",")
        ),
    )
    .unwrap();
    let e = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store#file:src/big.rs");
    assert_eq!(e.state, State::Missing, "{}", e.detail);
    assert!(e.detail.ends_with("and 5 more"), "{}", e.detail);
}

fn run(dir: &Path, outcomes: &[(&str, &str)]) {
    let results = dir.join("out/results");
    let _ = fs::remove_dir_all(&results);
    fs::create_dir_all(&results).unwrap();
    for (id, outcome) in outcomes {
        fs::write(
            results.join(format!("{id}.json")),
            format!(r#"{{"id":"{id}","outcome":"{outcome}"}}"#),
        )
        .unwrap();
    }
}

fn whole(dir: &Path) -> adsertio::verdict::Evidence {
    adsertio::probatio::evaluate(dir, "probatio:out")
}

#[test]
fn probatio_flaky_and_error_results_block_a_pass() {
    let dir = project("r4-prob-block");
    for odd in ["flaky", "error"] {
        run(&dir, &[("m1", "killed"), ("m2", odd)]);
        let e = whole(&dir);
        assert_eq!(e.state, State::Inconclusive, "{odd}: {}", e.detail);
    }
}

#[test]
fn an_outcome_probatio_does_not_write_is_not_proof() {
    let dir = project("r4-prob-odd");
    for odd in ["Survived", "survived ", "maybe"] {
        run(&dir, &[("m1", "killed"), ("m2", odd)]);
        let e = whole(&dir);
        assert_eq!(e.state, State::Unreadable, "{odd:?}: {}", e.detail);
        let one = adsertio::probatio::evaluate(&dir, "probatio:out#mutant:m2");
        assert_eq!(one.state, State::Unreadable, "{odd:?}: {}", one.detail);
    }
    run(&dir, &[("m1", "killed"), ("m2", "survived")]);
    assert_eq!(whole(&dir).state, State::Fail);
}

#[test]
fn an_unfinished_probatio_run_is_inconclusive() {
    let dir = project("r4-prob-partial");
    let patches = dir.join("out/generate");
    fs::create_dir_all(&patches).unwrap();
    fs::write(
        patches.join("mutants.json"),
        r#"{"schemaVersion":1,"mutants":[{"id":"m1"},{"id":"m2"},{"id":"m3"},{"id":"m4"}]}"#,
    )
    .unwrap();
    let canon = patches.canonicalize().unwrap();
    fs::write(
        dir.join("out/run.json"),
        format!(
            r#"{{"patches":["{}"]}}"#,
            canon.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    run(&dir, &[("m1", "killed")]);
    let e = whole(&dir);
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("1 of 4"), "{}", e.detail);
    run(
        &dir,
        &[
            ("m1", "killed"),
            ("m2", "killed"),
            ("m3", "timeout"),
            ("m4", "killed"),
        ],
    );
    let e = whole(&dir);
    assert_eq!(e.state, State::Pass, "{}", e.detail);
    // A broken run.json cannot say the run finished.
    fs::write(dir.join("out/run.json"), "{").unwrap();
    assert_eq!(whole(&dir).state, State::Inconclusive);
}

#[test]
fn a_one_part_relative_adsertio_dir_resolves_pointers() {
    let dir = project("r4-dir");
    init(&dir);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(
        dir.join(".trammel/store"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":false}}],"constraints":[],"failures":[]}"#,
    )
    .unwrap();
    let cli = |args: &[&str]| {
        Command::new(CLI)
            .args(args)
            .current_dir(&dir)
            .env("ADSERTIO_DIR", ".adsertio")
            .output()
            .unwrap()
    };
    let made = cli(&[
        "claim",
        "--summary",
        "step one",
        "--evidence",
        "trammel:.trammel/store#plan-1/step-1",
    ]);
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let viewed = String::from_utf8_lossy(&cli(&["view"]).stdout).into_owned();
    assert!(viewed.contains("\"state\":\"fail\""), "{viewed}");
}

#[test]
fn a_cursor_from_a_hand_written_time_can_be_followed() {
    let dir = project("r4-cursor");
    init(&dir);
    for (id, created) in [
        ("a-hand", "2026-11-25 01:00:00"),
        ("b-later", "2026-11-25T02:00:00Z"),
    ] {
        fs::write(
            dir.join(format!(".adsertio/claims/{id}.md")),
            format!("---\nsummary: {id}\nauthor: main\ncreated: {created}\n---\n"),
        )
        .unwrap();
    }
    let view = |args: &[&str]| {
        let out = Command::new(CLI)
            .arg("view")
            .args(args)
            .current_dir(&dir)
            .env_remove("ADSERTIO_DIR")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let first = view(&["--limit", "1"]);
    let cursor = first
        .split("\"next_after\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("{first}"))
        .to_string();
    let second = view(&["--limit", "1", "--after", &cursor]);
    assert!(second.contains("b-later"), "{second}");
    assert!(!second.contains("a-hand"), "{second}");
}

#[cfg(unix)]
#[test]
fn the_trammel_journal_is_read_beside_a_symlinked_store() {
    let dir = project("r4-trammel-link");
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::create_dir_all(dir.join("snap")).unwrap();
    fs::write(
        dir.join("snap/store"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"at":4102444800}}],"constraints":[],"failures":[]}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(dir.join("snap/store"), dir.join(".trammel/store")).unwrap();
    fs::write(
        dir.join(".trammel/journal"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":false,"at":4102444900}}],"constraints":[],"failures":[]}"#,
    )
    .unwrap();
    let e = adsertio::trammel::evaluate(&dir, "trammel:.trammel/store#plan-1/step-1");
    assert_eq!(e.state, State::Fail, "{}", e.detail);
}
