//! Claims, evidence pointers, and the verdict. Evidence is read from the
//! project; nothing outside it is touched; nothing unreadable counts as proof.
mod common;
use common::*;
use std::fs;
use std::process::Command;

#[test]
fn a_claim_without_evidence_is_unverified_and_says_why() {
    let dir = project("noevidence");
    init(&dir);
    let mut input = call(
        "adsertio_claim",
        r#"{"summary":"The retry change passes","author":"main","body":"Detail."}"#,
        1,
    );
    input += &call("adsertio_view", "{}", 2);
    let r = talk(&dir, input.as_bytes());
    assert!(is_ok(&r[0]) && is_ok(&r[1]), "{r:?}");
    assert!(
        r[1].contains("unverified") && r[1].contains("\\\"unverified\\\":1"),
        "{}",
        r[1]
    );
    assert!(strays(&dir).is_empty());
}

#[test]
fn evidence_states_are_reported_per_pointer_and_never_count_as_proof_yet() {
    let dir = project("states");
    init(&dir);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(dir.join(".chisel/store"), "{}").unwrap();
    fs::write(dir.join("binary.dat"), [0xffu8, 0xfe]).unwrap();
    let outside = project("states-outside");
    fs::write(outside.join("secret.json"), "{}").unwrap();
    let mut input = call(
        "adsertio_claim",
        r#"{"summary":"Everything checks out","id":"all","evidence":["chisel:.chisel/store","trammel:.trammel/store","probatio:binary.dat","probatio:../nope.json","chisel:.chisel/store#tests"]}"#,
        1,
    );
    input += &call("adsertio_view", r#"{"id":"all"}"#, 2);
    input += &call("adsertio_view", r#"{"verdict":"verified"}"#, 3);
    let r = talk(&dir, input.as_bytes());
    assert!(is_ok(&r[0]), "{}", r[0]);
    assert!(
        r[1].contains("missing") && r[1].contains("unreadable") && r[1].contains("magic"),
        "{}",
        r[1]
    );
    assert!(
        !r[1].contains("\\\"state\\\":\\\"pass\\\"")
            && !r[1].contains("\\\"state\\\":\\\"fail\\\""),
        "{}",
        r[1]
    );
    assert!(
        r[1].contains("\\\"verdict\\\":\\\"unverified\\\""),
        "{}",
        r[1]
    );
    assert!(r[2].contains("\\\"claims\\\":[]"), "{}", r[2]);
}

#[test]
fn symlinks_cannot_smuggle_outside_files_in() {
    let dir = project("symlink");
    init(&dir);
    let outside = project("symlink-outside");
    fs::write(outside.join("secret.json"), "{\"x\":1}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
    let mut input = call(
        "adsertio_claim",
        r#"{"summary":"s","evidence":["chisel:link/secret.json"]}"#,
        1,
    );
    input += &call("adsertio_view", "{}", 2);
    let r = talk(&dir, input.as_bytes());
    assert!(r[1].contains("unreadable") || !cfg!(unix), "{}", r[1]);
    assert!(!r[1].contains("no_adapter"));
}

#[test]
fn a_claim_is_never_overwritten() {
    let dir = project("nooverwrite");
    init(&dir);
    let a = call("adsertio_claim", r#"{"summary":"first","id":"pick"}"#, 1);
    let b = call("adsertio_claim", r#"{"summary":"second","id":"pick"}"#, 2);
    let r = talk(&dir, format!("{a}{b}").as_bytes());
    assert!(
        is_ok(&r[0]) && is_error(&r[1]) && r[1].contains("already exists"),
        "{r:?}"
    );
    assert!(fs::read_to_string(dir.join(".adsertio/claims/pick.md"))
        .unwrap()
        .contains("summary: first"));
    assert!(strays(&dir).is_empty());
}

#[test]
fn a_corrupt_claim_is_an_error_not_an_empty_view() {
    let dir = project("corrupt");
    init(&dir);
    talk(
        &dir,
        call("adsertio_claim", r#"{"summary":"fine","id":"fine"}"#, 1).as_bytes(),
    );
    let path = dir.join(".adsertio/claims/fine.md");
    let before = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        before.replace("summary: fine", "verdict: verified\nsummary: fine"),
    )
    .unwrap();
    let r = talk(&dir, call("adsertio_view", "{}", 1).as_bytes());
    assert!(
        is_error(&r[0]) && r[0].contains("corrupt claim"),
        "{}",
        r[0]
    );
    let out = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    fs::write(&path, [0xffu8, 0x00]).unwrap();
    let r = talk(&dir, call("adsertio_view", "{}", 1).as_bytes());
    assert!(is_error(&r[0]) && r[0].contains("UTF-8"), "{}", r[0]);
}

#[test]
fn concurrent_writers_lose_nothing() {
    let dir = project("concurrent");
    init(&dir);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                for j in 0..5 {
                    let out = Command::new(CLI)
                        .args(["claim", "--summary", &format!("claim {i}-{j}")])
                        .current_dir(&dir)
                        .output()
                        .unwrap();
                    assert!(
                        out.status.success(),
                        "{}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let out = Command::new(CLI)
        .args(["view"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"unverified\":40"));
    assert!(strays(&dir).is_empty());
}
