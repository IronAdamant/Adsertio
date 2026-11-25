//! Regressions from the 1.0.0 and 1.1.0 review.
mod common;
use common::*;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

#[test]
fn parallel_attaches_keep_every_successful_pointer() {
    let dir = project("parallel-attach");
    init(&dir);
    let made = Command::new(CLI)
        .args(["claim", "--summary", "Held", "--id", "held"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let mut children = Vec::new();
    for n in 0..12 {
        let pointer = format!("trammel:.trammel/store#plan-{n}");
        let child = Command::new(CLI)
            .args(["attach", "--id", "held", "--evidence", &pointer])
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        children.push((n, child));
    }
    let text_path = dir.join(".adsertio/claims/held.md");
    for (n, child) in children {
        let out = child.wait_with_output().unwrap();
        let msg = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let text = std::fs::read_to_string(&text_path).unwrap();
        let pointer = format!("#plan-{n}");
        if out.status.success() {
            assert!(
                text.contains(&pointer),
                "attach {n} exited 0 but {pointer} is missing\n{text}"
            );
        } else {
            assert!(
                msg.contains("another attach") && msg.contains("nothing was written"),
                "attach {n} failed without a conflict: {msg}"
            );
            assert!(!text.contains(&pointer), "failed attach {n} still wrote");
        }
    }
    let text = std::fs::read_to_string(&text_path).unwrap();
    let kept = (0..12)
        .filter(|n| text.contains(&format!("#plan-{n}")))
        .count();
    assert_eq!(kept, 12, "only {kept} of 12 pointers kept\n{text}");
}

#[test]
fn a_stale_claim_lock_is_replaced_and_a_fresh_one_conflicts() {
    let dir = project("stale-lock");
    init(&dir);
    let made = Command::new(CLI)
        .args(["claim", "--summary", "Held", "--id", "held"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let lock = dir.join(".adsertio/claims/.held.md.lock");
    std::fs::write(&lock, "dead\n").unwrap();
    let old = SystemTime::now() - Duration::from_secs(120);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let stole = Command::new(CLI)
        .args([
            "attach",
            "--id",
            "held",
            "--evidence",
            "trammel:.trammel/store#plan-stale",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        stole.status.success(),
        "{}",
        String::from_utf8_lossy(&stole.stderr)
    );
    assert!(!lock.exists(), "stale lock was left behind");
    let body = std::fs::read_to_string(dir.join(".adsertio/claims/held.md")).unwrap();
    assert!(body.contains("#plan-stale"), "{body}");

    std::fs::write(&lock, "live\n").unwrap();
    let before = body;
    let blocked = Command::new(CLI)
        .args([
            "attach",
            "--id",
            "held",
            "--evidence",
            "trammel:.trammel/store#plan-live",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&blocked.stdout),
        String::from_utf8_lossy(&blocked.stderr)
    );
    assert!(
        msg.contains("another attach") && msg.contains("nothing was written"),
        "{msg}"
    );
    let after = std::fs::read_to_string(dir.join(".adsertio/claims/held.md")).unwrap();
    assert_eq!(before, after);
    assert!(lock.exists());
    std::fs::remove_file(&lock).unwrap();
}

#[test]
fn omitting_limit_returns_every_claim_and_ids_sort_numerically() {
    let dir = project("all-claims");
    init(&dir);
    let claims = dir.join(".adsertio/claims");
    for n in 0..132 {
        let body = format!(
            "---\nsummary: Claim {n}\nauthor: agent\ncreated: 2020-01-01T00:00:00Z\nevidence: []\n---\n"
        );
        std::fs::write(claims.join(format!("c{n}.md")), body).unwrap();
    }
    let out = Command::new(CLI)
        .args(["ls"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"count\":132"), "{text}");
    assert!(text.contains("\"truncated\":false"), "{text}");
    assert!(text.contains("132 claims"), "{text}");
    let at = |id: &str| text.find(&format!("\"id\":\"{id}\"")).unwrap();
    assert!(at("c2") < at("c10") && at("c10") < at("c100"), "{text}");

    let viewed = Command::new(CLI)
        .args(["view"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        viewed.status.success(),
        "{}",
        String::from_utf8_lossy(&viewed.stderr)
    );
    let view = String::from_utf8_lossy(&viewed.stdout);
    assert!(view.contains("\"unverified\":132"), "{view}");
    assert!(view.contains("\"id\":\"c131\""), "{view}");
}

#[test]
fn pointers_that_cannot_resolve_are_rejected_at_write() {
    let dir = project("bad-pointer");
    init(&dir);
    let cases = [
        (
            r#"{"summary":"s","id":"bad","evidence":["trammel:.trammel\\store"]}"#,
            "backslashes",
        ),
        (
            r#"{"summary":"s","id":"bad","evidence":["trammel:C:/store"]}"#,
            "drive letter",
        ),
        (
            r#"{"summary":"s","id":"bad","evidence":["trammel:/etc/passwd"]}"#,
            "absolute",
        ),
        (
            r#"{"summary":"s","id":"bad","evidence":["trammel:../secret"]}"#,
            "..",
        ),
    ];
    for (args, why) in cases {
        let reply = talk(&dir, call("adsertio_claim", args, 1).as_bytes());
        assert!(is_error(&reply[0]), "{why}: {}", reply[0]);
        assert!(
            reply[0].contains("forward slashes") && reply[0].contains(why),
            "{why}: {}",
            reply[0]
        );
        assert!(!dir.join(".adsertio/claims/bad.md").exists(), "{why}");
    }
    let made = talk(
        &dir,
        call(
            "adsertio_claim",
            r#"{"summary":"ok","id":"ok","evidence":["trammel:.trammel/store"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&made[0]), "{}", made[0]);
    let before = std::fs::read_to_string(dir.join(".adsertio/claims/ok.md")).unwrap();
    let attached = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"ok","evidence":["chisel:D:/evidence"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&attached[0]) && attached[0].contains("drive letter"),
        "{}",
        attached[0]
    );
    assert_eq!(
        before,
        std::fs::read_to_string(dir.join(".adsertio/claims/ok.md")).unwrap()
    );
}

#[test]
fn limit_schema_minimum_is_one() {
    let dir = project("limit-schema");
    let reply = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    );
    assert!(
        reply[0].contains("\"limit\":{\"type\":\"integer\",\"minimum\":1"),
        "{}",
        reply[0]
    );
}

#[test]
fn a_folder_with_no_claims_directory_says_to_init() {
    let dir = project("no-claims-dir");
    let missing = Command::new(CLI)
        .arg("ls")
        .env("ADSERTIO_DIR", &dir)
        .output()
        .unwrap();
    assert!(
        missing.status.success(),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    let err = String::from_utf8_lossy(&missing.stderr);
    assert!(
        err.contains("no claims store found at") && err.contains("run init"),
        "{err}"
    );
    assert!(err.contains(&dir.display().to_string()), "{err}");
    let ready = Command::new(CLI)
        .arg("init")
        .env("ADSERTIO_DIR", &dir)
        .output()
        .unwrap();
    assert!(
        ready.status.success(),
        "{}",
        String::from_utf8_lossy(&ready.stderr)
    );
    let quiet = Command::new(CLI)
        .arg("ls")
        .env("ADSERTIO_DIR", &dir)
        .output()
        .unwrap();
    assert!(
        quiet.status.success(),
        "{}",
        String::from_utf8_lossy(&quiet.stderr)
    );
    let err = String::from_utf8_lossy(&quiet.stderr);
    assert!(!err.contains("no claims store found"), "{err}");
}

#[test]
fn view_can_filter_states_and_drop_detail() {
    let dir = project("states-compact");
    init(&dir);
    for id in ["one", "two"] {
        let made = talk(
            &dir,
            call(
                "adsertio_claim",
                &format!(
                    r#"{{"summary":"{id}","id":"{id}","evidence":["trammel:.trammel/store"]}}"#
                ),
                1,
            )
            .as_bytes(),
        );
        assert!(is_ok(&made[0]), "{}", made[0]);
    }
    let both = talk(
        &dir,
        call(
            "adsertio_view",
            r#"{"verdict":"unverified","states":["unverified"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&both[0]) && both[0].contains("not both"),
        "{}",
        both[0]
    );
    let bad = talk(
        &dir,
        call("adsertio_view", r#"{"states":["passed"]}"#, 1).as_bytes(),
    );
    assert!(
        is_error(&bad[0]) && bad[0].contains("states must be"),
        "{}",
        bad[0]
    );

    let filtered = talk(
        &dir,
        call("adsertio_view", r#"{"states":["verified","refuted"]}"#, 1).as_bytes(),
    );
    assert!(is_ok(&filtered[0]), "{}", filtered[0]);
    assert!(filtered[0].contains(r#"\"claims\":[]"#), "{}", filtered[0]);
    assert!(
        filtered[0].contains(r#"\"unverified\":2"#),
        "{}",
        filtered[0]
    );

    let kept = talk(
        &dir,
        call("adsertio_view", r#"{"states":["unverified"]}"#, 1).as_bytes(),
    );
    assert!(
        kept[0].contains("one") && kept[0].contains("two"),
        "{}",
        kept[0]
    );

    let full = talk(&dir, call("adsertio_view", r#"{"id":"one"}"#, 1).as_bytes());
    assert!(full[0].contains(r#"\"detail\""#), "{}", full[0]);
    let compact = talk(
        &dir,
        call("adsertio_view", r#"{"id":"one","compact":true}"#, 1).as_bytes(),
    );
    assert!(is_ok(&compact[0]), "{}", compact[0]);
    assert!(compact[0].contains(r#"\"state\""#), "{}", compact[0]);
    assert!(!compact[0].contains(r#"\"detail\""#), "{}", compact[0]);
}

#[test]
fn the_two_binaries_do_not_share_main_rs() {
    let toml = include_str!("../Cargo.toml");
    assert_eq!(toml.matches("path = \"src/main.rs\"").count(), 1);
    assert!(toml.contains("path = \"src/bin/adsertio-mcp.rs\""));
}

#[test]
fn the_ledger_row_is_not_reported_as_a_speedup() {
    let doc = include_str!("../docs/perf/1.1.0.md");
    assert!(doc.contains("No ledger code changed"));
    assert!(!doc.contains("| Ledger, 10,000 entries |"));
}

#[test]
fn future_archives_use_the_target_triple_and_forward_slashes() {
    let workflow = include_str!("../.github/workflows/release.yml");
    assert!(workflow.contains("adsertio-${version}-${TARGET}"));
    assert!(!workflow.contains("Compress-Archive"));
    let script = include_str!("../.github/scripts/zip-forward.ps1");
    assert!(script.contains("-replace '\\\\', '/'"));
}
