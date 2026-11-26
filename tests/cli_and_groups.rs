mod common;
use common::*;
use std::fs;
use std::process::Command;

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap()
}

#[test]
fn cli_rejects_bad_flags_with_exit_2() {
    let dir = project("cliflags");
    init(&dir);
    assert_eq!(cli(&dir, &["claim", "--summary"]).status.code(), Some(2));
    assert_eq!(cli(&dir, &["view", "--id"]).status.code(), Some(2));
    assert_eq!(cli(&dir, &["frobnicate"]).status.code(), Some(2));
    assert_eq!(cli(&dir, &[]).status.code(), Some(2));
    assert_eq!(
        cli(&dir, &["view", "--colour", "red"]).status.code(),
        Some(2)
    );
    assert_eq!(
        cli(&dir, &["claim", "--summary", "s", "--summary", "t"])
            .status
            .code(),
        Some(2)
    );
    assert!(cli(&dir, &["--version"]).status.success());
}

#[test]
fn cli_claim_and_view_round_trip() {
    let dir = project("cliview");
    init(&dir);
    assert!(cli(
        &dir,
        &[
            "claim",
            "--summary",
            "Tests pass",
            "--evidence",
            "chisel:a.json#test:t1",
            "--evidence",
            "probatio:b.json"
        ]
    )
    .status
    .success());
    let out = cli(&dir, &["view"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("Tests pass") && text.contains("\"missing\""),
        "{text}"
    );
    assert!(cli(&dir, &["check"]).status.success());
}

fn tools_list(dir: &std::path::Path) -> String {
    talk(
        dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    )
    .remove(0)
}

#[test]
fn agent_tools_typos_fall_back_to_core_with_a_notice() {
    let dir = project("groups");
    init(&dir);
    assert!(tools_list(&dir).contains("adsertio_view"));
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nrelatum: disable lint\nadsertio: disable verify\n```\n",
    )
    .unwrap();
    let l = tools_list(&dir);
    assert!(
        l.contains("NOTICE") && l.contains("unknown group") && l.contains("adsertio_claim"),
        "{l}"
    );
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nadsertio: disable core\n```\n",
    )
    .unwrap();
    assert!(tools_list(&dir).contains("cannot be disabled"));
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nrelatum: disable lint\n```\n",
    )
    .unwrap();
    let l = tools_list(&dir);
    assert!(!l.contains("NOTICE") && l.contains("adsertio_view"));
}

#[test]
fn initialize_reports_the_server() {
    let dir = project("init");
    let r = talk(&dir, b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n");
    assert!(
        r[0].contains("\"name\":\"adsertio\"") && r[0].contains("2025-06-18"),
        "{}",
        r[0]
    );
}
