//! Regression tests for 1.2.2. The rule: when Adsertio
//! cannot show that a run is complete, or that a result is the latest, the
//! answer is not a pass. Most fixtures here were written by the real tools
//! (`probatio mutate run`, `probatio mutate diff --max-mutants 1`,
//! `chisel analyze` with shards, `chisel run` with no commit), with absolute
//! paths in them replaced by `/project`.
mod common;
use adsertio::verdict::State;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

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

fn cli(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap()
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn write_claim(dir: &Path, id: &str, created: &str, evidence: &str) {
    let ev = if evidence.is_empty() {
        String::new()
    } else {
        format!("evidence: [{evidence}]\n")
    };
    fs::write(
        dir.join(format!(".adsertio/claims/{id}.md")),
        format!("---\nsummary: {id}\nauthor: agent\ncreated: {created}\n{ev}---\n"),
    )
    .unwrap();
}

/// The real Probatio output under `<project>/.probatio`.
fn real_probatio(name: &str) -> PathBuf {
    let dir = project(name);
    copy_tree(&fixtures().join("probatio/real"), &dir.join(".probatio"));
    dir
}

fn probatio(dir: &Path, pointer: &str) -> adsertio::verdict::Evidence {
    adsertio::probatio::evaluate(dir, pointer)
}

// A. A `mutate diff` call capped by --max-mutants 1 scored 1 of 3 mutants.
// Its generate/mutants.json lists only that one, and nothing Probatio writes
// names the other two, so the run is never a pass.
#[test]
fn a_capped_probatio_diff_run_is_inconclusive() {
    let dir = real_probatio("r5-diff");
    let generate = dir.join(".probatio/diff/generate");
    fs::write(
        dir.join(".probatio/diff/run/run.json"),
        format!(
            "{{\"package\":\".\",\"patches\":[\"{}\"],\"out\":\"{}\"}}",
            generate.display(),
            dir.join(".probatio/diff/run").display()
        ),
    )
    .unwrap();
    let e = probatio(&dir, "probatio:.probatio/diff/run");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("1 killed"), "{}", e.detail);
    assert!(e.detail.contains("mutate diff"), "{}", e.detail);
    // One mutant on its own is still judged.
    let one = probatio(&dir, "probatio:.probatio/diff/run#mutant:m3f67ab1630ef");
    assert_eq!(one.state, State::Pass, "{}", one.detail);
}

// B. A real `mutate run` whose run.json names ".probatio/slice", relative to
// the folder Probatio ran in, still passes.
#[test]
fn a_real_probatio_run_with_a_relative_patches_path_passes() {
    let dir = real_probatio("r5-real-run");
    let e = probatio(&dir, "probatio:.probatio/run");
    assert_eq!(e.state, State::Pass, "{}", e.detail);
    assert!(e.detail.contains("2 killed"), "{}", e.detail);

    // The slice deleted: its list can no longer be checked.
    fs::remove_dir_all(dir.join(".probatio/slice")).unwrap();
    let gone = probatio(&dir, "probatio:.probatio/run");
    assert_eq!(gone.state, State::Inconclusive, "{}", gone.detail);
    assert!(gone.detail.contains("cannot tell"), "{}", gone.detail);
}

// B. Probatio's own tally says a `no coverage` mutant is not a pass.
#[test]
fn a_no_coverage_mutant_blocks_a_pass() {
    let dir = real_probatio("r5-nocov");
    let e = probatio(&dir, "probatio:.probatio/nocov");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("1 killed"), "{}", e.detail);
    assert!(e.detail.contains("no coverage"), "{}", e.detail);
}

fn hand_run(name: &str) -> PathBuf {
    let dir = project(name);
    fs::create_dir_all(dir.join("out/results")).unwrap();
    for id in ["m1", "m2"] {
        fs::write(
            dir.join(format!("out/results/{id}.json")),
            format!("{{\"id\":\"{id}\",\"outcome\":\"killed\"}}"),
        )
        .unwrap();
    }
    dir
}

// B. A patches folder with one file per mutant and no mutants.json: Probatio
// reads every *.json in it, and so does Adsertio.
#[test]
fn a_patches_folder_without_mutants_json_is_read_like_probatio() {
    let dir = hand_run("r5-pfiles");
    fs::create_dir_all(dir.join("patches")).unwrap();
    for id in ["m1", "m2", "m3"] {
        fs::write(
            dir.join(format!("patches/{id}.json")),
            format!("{{\"mutants\":[{{\"id\":\"{id}\"}}]}}"),
        )
        .unwrap();
    }
    fs::write(dir.join("out/run.json"), r#"{"patches":["patches"]}"#).unwrap();
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("2 of 3"), "{}", e.detail);

    fs::write(
        dir.join("out/results/m3.json"),
        r#"{"id":"m3","outcome":"killed"}"#,
    )
    .unwrap();
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Pass, "{}", e.detail);
}

// B. Every way the planned list can be out of reach is inconclusive.
#[test]
fn a_run_whose_list_cannot_be_checked_is_inconclusive() {
    // No run.json at all.
    let dir = hand_run("r5-norun");
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("run.json"), "{}", e.detail);

    // A list outside the project.
    let dir = hand_run("r5-outside");
    let outside = project("r5-outside-list");
    fs::write(
        outside.join("mutants.json"),
        r#"{"mutants":[{"id":"m1"},{"id":"m2"}]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("out/run.json"),
        format!(
            "{{\"patches\":[\"{}\"]}}",
            outside.join("mutants.json").display()
        ),
    )
    .unwrap();
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("outside the project"), "{}", e.detail);

    // A patches folder that is gone.
    let dir = hand_run("r5-gone");
    fs::write(dir.join("out/run.json"), r#"{"patches":["patches"]}"#).unwrap();
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);

    // No patches named.
    let dir = hand_run("r5-nopatches");
    fs::write(dir.join("out/run.json"), r#"{"patches":[]}"#).unwrap();
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);

    // Two --patches entries: the second list's mutant has no result.
    let dir = hand_run("r5-two");
    fs::create_dir_all(dir.join("a")).unwrap();
    fs::create_dir_all(dir.join("b")).unwrap();
    fs::write(
        dir.join("a/mutants.json"),
        r#"{"mutants":[{"id":"m1"},{"id":"m2"}]}"#,
    )
    .unwrap();
    fs::write(dir.join("b/mutants.json"), r#"[{"id":"m3"}]"#).unwrap();
    fs::write(dir.join("out/run.json"), r#"{"patches":["a","./b/"]}"#).unwrap();
    let e = probatio(&dir, "probatio:out");
    assert_eq!(e.state, State::Inconclusive, "{}", e.detail);
    assert!(e.detail.contains("2 of 3"), "{}", e.detail);
}

/// The real sharded Chisel store: shards `src/net` and `src/web` are nested
/// folders, and the root holds every test result (a failure, then a pass).
fn real_shards(name: &str) -> PathBuf {
    let dir = project(name);
    copy_tree(
        &fixtures().join("chisel/real-sharded"),
        &dir.join(".chisel"),
    );
    dir
}

// C. Chisel records results only in the root store. An old shard row (here
// a failure with a later time) must not override the root's latest pass.
#[test]
fn a_shard_result_never_overrides_the_root() {
    let dir = real_shards("r5-shard");
    let root = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store#test:tests/a.rs:t1");
    assert_eq!(root.state, State::Pass, "{}", root.detail);

    let shard = dir.join(".chisel/shards/_unassigned/store");
    let body = fs::read_to_string(&shard).unwrap().replacen(
        "\"test_results\":[]",
        "\"test_results\":[{\"test_id\":\"tests/a.rs:t1\",\"passed\":false,\"recorded_at\":4102444800}]",
        1,
    );
    assert!(body.contains("4102444800"));
    fs::write(&shard, body).unwrap();
    let root = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store#test:tests/a.rs:t1");
    assert_eq!(root.state, State::Pass, "{}", root.detail);

    // A pointer at one shard is not proof either way.
    let one = adsertio::chisel::evaluate(
        &dir,
        "chisel:.chisel/shards/_unassigned/store#test:tests/a.rs:t1",
    );
    assert_eq!(one.state, State::NoAdapter, "{}", one.detail);
}

// C. A nested shard (`.chisel/shards/src/net/store`) still adds its edges.
#[test]
fn a_nested_shard_adds_its_test_edges() {
    let dir = real_shards("r5-nested");
    let shard = dir.join(".chisel/shards/src/net/store");
    let body = fs::read_to_string(&shard).unwrap().replacen(
        "\"test_edges\":[]",
        "\"test_edges\":[{\"test_id\":\"tests/a.rs:t1\",\"code_id\":\"src/net/c.rs:c:function\",\"edge_type\":\"call\",\"weight\":1}]",
        1,
    );
    fs::write(&shard, body).unwrap();
    let e = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store#file:src/net/c.rs");
    assert_eq!(e.state, State::Pass, "{}", e.detail);
}

fn overlay(name: &str) -> PathBuf {
    let dir = project(name);
    fs::create_dir_all(dir.join(".chisel/overlays/wt")).unwrap();
    fs::write(
        dir.join(".chisel/overlays/wt/store"),
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"t/a.rs:t1","passed":true}],"test_edges":[]}"#,
    )
    .unwrap();
    dir
}

// D. The overlay guard holds for another letter case and for a symlink.
#[test]
fn an_overlay_is_not_read_however_it_is_reached() {
    let dir = overlay("r5-ovl");
    for pointer in [
        "chisel:.chisel/overlays/wt/store#test:t/a.rs:t1",
        "chisel:.CHISEL/OVERLAYS/wt/store#test:t/a.rs:t1",
        "chisel:.Chisel/Overlays/wt/store#test:t/a.rs:t1",
    ] {
        let e = adsertio::chisel::evaluate(&dir, pointer);
        assert_eq!(e.state, State::NoAdapter, "{pointer}: {}", e.detail);
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_into_the_overlays_is_not_read() {
    let dir = overlay("r5-ovl-link");
    std::os::unix::fs::symlink(dir.join(".chisel/overlays/wt"), dir.join("snap")).unwrap();
    let e = adsertio::chisel::evaluate(&dir, "chisel:snap/store#test:t/a.rs:t1");
    assert_eq!(e.state, State::NoAdapter, "{}", e.detail);
    std::os::unix::fs::symlink(
        dir.join(".chisel/overlays/wt/store"),
        dir.join("store-link"),
    )
    .unwrap();
    let e = adsertio::chisel::evaluate(&dir, "chisel:store-link#test:t/a.rs:t1");
    assert_eq!(e.state, State::NoAdapter, "{}", e.detail);
}

fn next_after(out: &str) -> Option<String> {
    let start = out.find("\"next_after\":\"")? + "\"next_after\":\"".len();
    let end = out[start..].find('"')? + start;
    Some(out[start..end].to_string())
}

// E. Two ids whose number runs are equal (`a01b1`, `a1b01`) in the same
// second: paging one at a time reaches each exactly once.
#[test]
fn paging_never_skips_ids_that_differ_only_in_leading_zeros() {
    let dir = project("r5-page");
    init(&dir);
    for id in ["a1b01", "a01b1", "a1b1", "a001b1", "b2"] {
        write_claim(&dir, id, "2026-01-01T00:00:00Z", "");
    }
    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..10 {
        let mut args = vec!["view", "--compact", "--limit", "1"];
        let cursor;
        if let Some(a) = &after {
            cursor = a.clone();
            args.push("--after");
            args.push(&cursor);
        }
        let out = cli(&dir, &args);
        assert!(out.status.success(), "{}", text(&out));
        let body = String::from_utf8_lossy(&out.stdout).to_string();
        for id in ["a1b01", "a01b1", "a1b1", "a001b1", "b2"] {
            if body.contains(&format!("\"id\":\"{id}\"")) {
                seen.push(id.to_string());
            }
        }
        match next_after(&body) {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(seen.len(), 5, "{seen:?}");
    assert_eq!(sorted.len(), 5, "{seen:?}");
}

// F. The command line: a repeated flag of any kind, and an unknown flag, are
// usage errors (exit 2). A value starting with -- goes after = or a lone --.
#[test]
fn cli_flags_are_strict_and_allow_dashed_values() {
    let dir = project("r5-cli");
    init(&dir);
    for args in [
        &["view", "--limit", "1", "--limit", "2"][..],
        &["view", "--compact", "--compact"][..],
        &["view", "--compact", "--compact", "false"][..],
        &["view", "--colour", "red"][..],
        &["view", "--colour"][..],
        &["ls", "--offset", "1", "--offset", "1"][..],
        &["view", "--compact=maybe"][..],
    ] {
        let out = cli(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
    }
    let out = cli(&dir, &["claim", "--summary=--fast path", "--id", "dash-a"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = cli(
        &dir,
        &["claim", "--summary", "--", "--slow path", "--id=dash-b"],
    );
    assert!(out.status.success(), "{}", text(&out));
    let a = fs::read_to_string(dir.join(".adsertio/claims/dash-a.md")).unwrap();
    assert!(a.contains("summary: --fast path"), "{a}");
    let b = fs::read_to_string(dir.join(".adsertio/claims/dash-b.md")).unwrap();
    assert!(b.contains("summary: --slow path"), "{b}");
    let out = cli(&dir, &["view", "--compact=true", "--limit=1"]);
    assert!(out.status.success(), "{}", text(&out));
}

// G. A symlinked claim is left out of view and ls with a warning, and check
// reports it. Check also reports a dotfile that is not Adsertio's own.
#[cfg(unix)]
#[test]
fn a_symlinked_claim_is_left_out_and_check_reports_it() {
    let dir = project("r5-link-claim");
    init(&dir);
    write_claim(&dir, "real", "2026-01-01T00:00:00Z", "");
    let outside = project("r5-link-claim-out");
    fs::write(
        outside.join("x.md"),
        "---\nsummary: x\nauthor: agent\ncreated: 2026-01-01T00:00:00Z\n---\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.join("x.md"), dir.join(".adsertio/claims/x.md")).unwrap();
    for cmd in ["view", "ls"] {
        let out = cli(&dir, &[cmd]);
        assert!(out.status.success(), "{cmd}: {}", text(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("\"real\""), "{stdout}");
        assert!(!stdout.contains("\"x\""), "{stdout}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("x.md"),
            "{}",
            text(&out)
        );
    }
    let out = cli(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("x.md is a symlink"), "{}", text(&out));
}

#[test]
fn check_reports_a_dotfile_in_the_claims_folder() {
    let dir = project("r5-dotfile");
    init(&dir);
    write_claim(&dir, "real", "2026-01-01T00:00:00Z", "");
    assert!(cli(&dir, &["check"]).status.success());
    fs::write(dir.join(".adsertio/claims/.notes.md"), "hello").unwrap();
    let out = cli(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains(".notes.md"), "{}", text(&out));
    let out = cli(&dir, &["view"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(".notes.md"), "{}", text(&out));
    fs::remove_file(dir.join(".adsertio/claims/.notes.md")).unwrap();
    // Finder's .DS_Store is not a stray either.
    fs::write(dir.join(".adsertio/claims/.DS_Store"), "x").unwrap();
    assert!(cli(&dir, &["check"]).status.success());
    let out = cli(&dir, &["view"]);
    assert!(!text(&out).contains(".DS_Store"), "{}", text(&out));
    // Adsertio's own lock guard is not a stray.
    fs::write(dir.join(".adsertio/claims/.real.md.lock.break"), "").unwrap();
    let out = cli(&dir, &["check"]);
    assert!(out.status.success(), "{}", text(&out));
}

// H. `chisel run` in a repository with no commit writes test_passes with
// commit "". That is no commit recorded, not a malformed one.
#[test]
fn an_empty_chisel_pass_commit_is_no_commit() {
    let dir = project("r5-empty-commit");
    init(&dir);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::copy(
        fixtures().join("chisel/real-empty-commit.json"),
        dir.join(".chisel/store"),
    )
    .unwrap();
    write_claim(
        &dir,
        "t1",
        "2026-01-01T00:00:00Z",
        "chisel:.chisel/store#test:tests/a.rs:t1",
    );
    let out = cli(&dir, &["check"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = cli(&dir, &["view", "--id", "t1"]);
    assert!(out.status.success(), "{}", text(&out));
    let body = text(&out);
    assert!(body.contains("\"verdict\":\"verified\""), "{body}");
    assert!(body.contains("no commit"), "{body}");
    // The pass still has to be newer than the claim.
    write_claim(
        &dir,
        "t1",
        "2030-01-01T00:00:00Z",
        "chisel:.chisel/store#test:tests/a.rs:t1",
    );
    let body = text(&cli(&dir, &["view", "--id", "t1"]));
    assert!(body.contains("\"state\":\"stale\""), "{body}");
}

// I. A Chisel store over 64 MiB is read (Chisel writes up to 1 GiB). Past
// the limit, the detail says the limit and why.
#[test]
fn a_large_chisel_store_is_read_and_the_limit_is_explained() {
    let dir = project("r5-big");
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    let mut body = String::from(
        "{\"magic\":\"chisel-store\",\"version\":1,\"test_results\":[{\"test_id\":\"t\",\"passed\":true}],\"test_edges\":[],\"pad\":\"",
    );
    body.push_str(&"x".repeat(65 * 1024 * 1024));
    body.push_str("\"}");
    fs::write(dir.join(".chisel/store"), body).unwrap();
    let e = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store#test:t");
    assert_eq!(e.state, State::Pass, "{}", e.detail);

    let big = fs::File::create(dir.join(".chisel/store")).unwrap();
    big.set_len(adsertio::read::MAX_STORE_BYTES + 1).unwrap();
    drop(big);
    let e = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store#test:t");
    assert_eq!(e.state, State::Unreadable, "{}", e.detail);
    assert!(e.detail.contains("1024 MiB limit"), "{}", e.detail);
    assert!(e.detail.contains("memory"), "{}", e.detail);
}

// J. A created time with a leading + is not YYYY-MM-DDTHH:MM:SSZ.
#[test]
fn a_created_time_with_a_plus_sign_is_rejected() {
    let dir = project("r5-plus");
    init(&dir);
    write_claim(&dir, "plus", "+026-01-01T00:00:00Z", "");
    let out = cli(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("YYYY-MM-DDTHH:MM:SSZ"),
        "{}",
        text(&out)
    );
}

// K. The AGENTS.md test list names every test file.
#[test]
fn agents_md_lists_every_test_file() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    for entry in fs::read_dir(root.join("tests")).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        if name.ends_with(".rs") {
            assert!(
                agents.contains(&format!("tests/{name}")),
                "AGENTS.md does not list tests/{name}"
            );
        }
    }
}
