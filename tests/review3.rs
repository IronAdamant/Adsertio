//! Third verification pass: each finding has a regression.
mod common;
use common::*;
use std::process::{Command, Stdio};

fn claim_file(summary: &str, created: &str, evidence: &str) -> String {
    format!(
        "---\nsummary: {summary}\nauthor: agent\ncreated: {created}\nevidence: [{evidence}]\n---\n"
    )
}

#[test]
fn a_truncated_hint_keeps_the_view_filters() {
    let dir = project("hint-filters");
    init(&dir);
    for id in ["a", "b"] {
        let made = talk(
            &dir,
            call(
                "adsertio_claim",
                &format!(r#"{{"summary":"{id}","id":"{id}"}}"#),
                1,
            )
            .as_bytes(),
        );
        assert!(is_ok(&made[0]), "{}", made[0]);
    }
    let page = talk(
        &dir,
        call(
            "adsertio_view",
            r#"{"limit":1,"verdict":"unverified","compact":true}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&page[0]), "{}", page[0]);
    assert!(
        page[0].contains(r#"\\\"verdict\\\":\\\"unverified\\\""#)
            && page[0].contains(r#"\\\"compact\\\":true"#)
            && page[0].contains(r#"\\\"limit\\\":1"#)
            && page[0].contains(r#"\\\"after\\\":\\\""#),
        "{}",
        page[0]
    );
    assert!(
        !page[0].contains(r#"\\\"offset\\\""#),
        "the hint uses after, not offset: {}",
        page[0]
    );

    let states = talk(
        &dir,
        call("adsertio_view", r#"{"limit":1,"states":["unverified"]}"#, 1).as_bytes(),
    );
    assert!(is_ok(&states[0]), "{}", states[0]);
    assert!(
        states[0].contains(r#"\\\"states\\\":[\\\"unverified\\\"]"#),
        "{}",
        states[0]
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_claims_folder_is_not_followed() {
    let dir = project("symlink-claims");
    init(&dir);
    let outside = project("symlink-outside");
    let target = outside.join("taken");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("secret.md"), "outside").unwrap();
    let claims = dir.join(".adsertio/claims");
    std::fs::remove_dir(&claims).unwrap();
    std::os::unix::fs::symlink(&target, &claims).unwrap();
    let listed = Command::new(CLI)
        .arg("ls")
        .current_dir(&dir)
        .output()
        .unwrap();
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&listed.stdout),
        String::from_utf8_lossy(&listed.stderr)
    );
    assert!(!listed.status.success(), "{msg}");
    assert!(msg.contains("symlink"), "{msg}");
    assert!(!msg.contains("secret"), "{msg}");
    let wrote = Command::new(CLI)
        .args(["claim", "--summary", "Nope", "--id", "nope"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!wrote.status.success());
    assert!(!target.join("nope.md").exists());
}

#[test]
fn a_badly_named_claim_is_skipped_and_check_flags_it() {
    let dir = project("bad-name");
    init(&dir);
    let body = claim_file("Hidden", "2020-01-01T00:00:00Z", "");
    std::fs::write(dir.join(".adsertio/claims/Bad Name.md"), body).unwrap();
    let listed = Command::new(CLI)
        .arg("ls")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let out = String::from_utf8_lossy(&listed.stdout);
    let err = String::from_utf8_lossy(&listed.stderr);
    assert!(!out.contains("Hidden"), "{out}");
    assert!(err.contains("Bad Name.md"), "{err}");
    let checked = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!checked.status.success());
    let msg = String::from_utf8_lossy(&checked.stderr);
    assert!(msg.contains("Bad Name.md"), "{msg}");
}

#[test]
fn check_rejects_a_bad_created_time() {
    let dir = project("bad-created");
    init(&dir);
    std::fs::write(
        dir.join(".adsertio/claims/when.md"),
        claim_file("When", "yesterday", ""),
    )
    .unwrap();
    let checked = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!checked.status.success());
    let msg = String::from_utf8_lossy(&checked.stderr);
    assert!(msg.contains("YYYY-MM-DDTHH:MM:SSZ"), "{msg}");
    let listed = Command::new(CLI)
        .arg("ls")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("when"));
}

#[test]
fn a_string_timestamp_or_empty_commit_does_not_pass() {
    let dir = project("bad-stamp");
    init(&dir);
    std::fs::create_dir_all(dir.join(".trammel")).unwrap();
    std::fs::write(
        dir.join(".trammel/store"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"status":"passed","verification":{"passed":true,"at":"soon","commit":""}}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join(".adsertio/claims/held.md"),
        claim_file(
            "Held",
            "2020-01-01T00:00:00Z",
            "trammel:.trammel/store#plan-1/step-1",
        ),
    )
    .unwrap();
    let viewed = talk(
        &dir,
        call("adsertio_view", r#"{"id":"held"}"#, 1).as_bytes(),
    );
    assert!(is_ok(&viewed[0]), "{}", viewed[0]);
    assert!(viewed[0].contains("malformed"), "{}", viewed[0]);
    assert!(viewed[0].contains("stale"), "{}", viewed[0]);
    assert!(
        !viewed[0].contains("\\\"state\\\":\\\"pass\\\""),
        "{}",
        viewed[0]
    );
    let checked = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stderr).contains("malformed"));
    let doctor = talk(&dir, call("adsertio_doctor", "{}", 1).as_bytes());
    assert!(
        doctor[0].contains("malformed timestamp or commit"),
        "{}",
        doctor[0]
    );
}

#[cfg(unix)]
#[test]
fn a_pipe_is_not_called_a_folder() {
    let dir = project("fifo");
    init(&dir);
    let pipe = dir.join("notes.pipe");
    let made = Command::new("mkfifo").arg(&pipe).status().unwrap();
    assert!(made.success());
    std::fs::write(
        dir.join(".adsertio/claims/held.md"),
        claim_file(
            "Held",
            "2020-01-01T00:00:00Z",
            "trammel:notes.pipe#plan-1/step-1",
        ),
    )
    .unwrap();
    let viewed = talk(
        &dir,
        call("adsertio_view", r#"{"id":"held"}"#, 1).as_bytes(),
    );
    assert!(viewed[0].contains("pipe"), "{}", viewed[0]);
    assert!(!viewed[0].contains("folder"), "{}", viewed[0]);
}

#[test]
fn attach_stops_at_fifty_pointers_and_duplicates_are_kept_once() {
    let dir = project("caps");
    init(&dir);
    let pointers: Vec<String> = (0..50)
        .map(|n| format!("trammel:.trammel/p{n}#plan-1"))
        .collect();
    let body = format!(
        "---\nsummary: Full\nauthor: agent\ncreated: 2020-01-01T00:00:00Z\nevidence: [{}]\n---\n",
        pointers.join(", ")
    );
    std::fs::write(dir.join(".adsertio/claims/full.md"), &body).unwrap();
    let extra = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"full","evidence":["trammel:.trammel/extra#plan-1"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&extra[0]) && extra[0].contains("50"),
        "{}",
        extra[0]
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(".adsertio/claims/full.md")).unwrap(),
        body
    );

    let made = talk(
        &dir,
        call(
            "adsertio_claim",
            r#"{"summary":"Once","id":"once","evidence":["trammel:.trammel/store#plan-1","trammel:.trammel/store#plan-1"],"tags":["net","net"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&made[0]), "{}", made[0]);
    let text = std::fs::read_to_string(dir.join(".adsertio/claims/once.md")).unwrap();
    assert_eq!(
        text.matches("trammel:.trammel/store#plan-1").count(),
        1,
        "{text}"
    );
    assert_eq!(text.matches("net").count(), 1, "{text}");
}

#[test]
fn parallel_claims_with_the_same_summary_all_get_ids() {
    let dir = project("parallel-ids");
    init(&dir);
    let mut children = Vec::new();
    for _ in 0..12 {
        children.push(
            Command::new(CLI)
                .args(["claim", "--summary", "Same summary"])
                .current_dir(&dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let out = child.wait_with_output().unwrap();
        let msg = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{msg}");
    }
    let count = std::fs::read_dir(dir.join(".adsertio/claims"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .ok()
                .and_then(|e| e.path().extension().map(|x| x == "md"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 12);
}

#[test]
fn check_removes_an_attach_lock_left_by_1_3() {
    let dir = project("stale-check");
    init(&dir);
    std::fs::write(
        dir.join(".adsertio/claims/held.md"),
        claim_file("Held", "2020-01-01T00:00:00Z", ""),
    )
    .unwrap();
    // 1.3 wrote the holder pid. A live one (this process) blocks nothing now.
    let lock = dir.join(".adsertio/claims/.held.md.lock");
    std::fs::write(&lock, format!("{} 0 0-0\n", std::process::id())).unwrap();
    let checked = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(!lock.exists(), "a 1.3 lock is removed by check");
}

#[test]
fn limit_errors_name_the_real_range() {
    let dir = project("limit-range");
    init(&dir);
    for args in [r#"{"limit":0}"#, r#"{"limit":501}"#, r#"{"limit":1.5}"#] {
        let reply = talk(&dir, call("adsertio_ls", args, 1).as_bytes());
        assert!(
            is_error(&reply[0]) && reply[0].contains("from 1 to 500"),
            "{args}: {}",
            reply[0]
        );
    }
}

#[test]
fn docs_match_the_two_binaries_and_the_unpaged_list() {
    let readme = include_str!("../README.md");
    assert!(!readme.contains("One binary"));
    assert!(!readme.contains("One page."));
    assert!(readme.contains("Each archive holds `adsertio` and `adsertio-mcp`"));
    assert!(readme.contains("With no limit, `view` and `ls` return every claim"));
    let log = include_str!("../LLM_Development.md");
    assert!(!log.contains("1.1.0 in the tree, on a branch"));
    assert!(log.contains("1.1.1"));
    assert!(log.contains("1.2.0"));
    let script = include_str!("../.github/scripts/zip-forward.ps1");
    assert!(script.contains("0x50") && script.contains("0x4B"));
    assert!(script.contains("adsertio.exe") && script.contains("adsertio-mcp.exe"));
    assert!(script.contains("[char]92"));
}

#[test]
fn jsonrpc_must_be_2_0_and_a_bare_folder_needs_init() {
    let dir = project("jsonrpc");
    let missing = talk(&dir, b"{\"id\":1,\"method\":\"ping\"}\n");
    assert!(
        missing[0].contains("jsonrpc") && missing[0].contains("2.0"),
        "{}",
        missing[0]
    );
    let wrong = talk(
        &dir,
        b"{\"jsonrpc\":\"1.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    assert!(wrong[0].contains("-32600"), "{}", wrong[0]);

    let bare = project("bare-claim");
    let claimed = Command::new(CLI)
        .args(["claim", "--summary", "Nope"])
        .env("ADSERTIO_DIR", &bare)
        .output()
        .unwrap();
    assert!(!claimed.status.success());
    let msg = String::from_utf8_lossy(&claimed.stderr);
    assert!(msg.contains("run init"), "{msg}");
    assert!(!bare.join("claims").exists());
}

/// 1.3.1 folded the `adsertio_check` tool into `adsertio_doctor`; the
/// `adsertio check` command stays for scripts and CI.
#[test]
fn doctor_does_what_the_check_tool_did() {
    let dir = project("mcp-check");
    init(&dir);
    let reply = talk(&dir, call("adsertio_doctor", "{}", 1).as_bytes());
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains("0 claims, none damaged"), "{}", reply[0]);
    let gone = talk(&dir, call("adsertio_check", "{}", 1).as_bytes());
    assert!(!is_ok(&gone[0]), "{}", gone[0]);
    let checked = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(checked.status.success());
}
