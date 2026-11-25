//! 1.3.0: batch claim and attach, the session hook, doctor, and the hint on
//! a pass that went stale because a commit moved HEAD.
mod common;
use adsertio::json::{self, Value};
use common::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn claim_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join(".adsertio/claims"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort();
    names
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn repo(name: &str) -> std::path::PathBuf {
    let dir = project(name);
    init(&dir);
    git(&dir, &["init"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "Test"]);
    fs::write(dir.join("tracked.txt"), "x").unwrap();
    git(&dir, &["add", "tracked.txt"]);
    git(&dir, &["commit", "-m", "init"]);
    dir
}

/// The text of the one tool result in an MCP reply line.
fn result_text(reply: &str) -> String {
    let v = json::parse(reply).unwrap();
    let content = v.get("result").unwrap().get("content").unwrap();
    content.as_array().unwrap()[0]
        .get("text")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

// Batch claim.

#[test]
fn a_batch_of_valid_claims_is_written_in_one_call() {
    let dir = project("v130-batch-ok");
    init(&dir);
    let args = r#"{"claims":[
        {"summary":"Retry passes","evidence":["trammel:.trammel/store#plan-1/step-1"]},
        {"summary":"Retry passes"},
        {"summary":"Parser holds","id":"parser","author":"main","tags":["net"],"evidence":["chisel:.chisel/store#test:t1"]}
    ]}"#
    .replace('\n', " ");
    let reply = talk(&dir, call("adsertio_claim", &args, 1).as_bytes());
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    let body = json::parse(&result_text(&reply[0])).unwrap();
    let ids: Vec<&str> = body
        .get("ids")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(ids, ["retry-passes", "retry-passes-2", "parser"]);
    assert_eq!(body.get("count").and_then(Value::as_u64), Some(3));
    assert_eq!(
        claim_files(&dir),
        ["parser.md", "retry-passes-2.md", "retry-passes.md"]
    );
    let parser = fs::read_to_string(dir.join(".adsertio/claims/parser.md")).unwrap();
    assert!(parser.contains("author: main"), "{parser}");
    assert!(parser.contains("tags: [net]"), "{parser}");
    let out = cli(&dir, &["check"]);
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn one_bad_item_writes_nothing_and_names_the_item() {
    let dir = project("v130-batch-bad");
    init(&dir);
    for (args, want) in [
        (
            r#"{"claims":[{"summary":"Fine"},{"summary":"Bad","evidence":["nope:x"]}]}"#,
            "claims item 2",
        ),
        (
            r#"{"claims":[{"summary":"Fine"},{"summary":"Bad","id":"Not A Slug"}]}"#,
            "claims item 2",
        ),
        (
            r#"{"claims":[{"summary":"Fine"},{"evidence":[]}]}"#,
            "claims item 2: missing required argument: summary",
        ),
        (
            r#"{"claims":[{"summary":"Fine","colour":"red"}]}"#,
            "claims item 1: unknown argument",
        ),
        (
            r#"{"claims":[{"summary":"Fine"},"text"]}"#,
            "item 2 must be an object",
        ),
        (
            r#"{"claims":[{"summary":"Fine","evidence":["trammel:../out#plan-1"]}]}"#,
            "claims item 1",
        ),
        (r#"{"claims":[]}"#, "claims is empty"),
        (r#"{"claims":[{"summary":"A"}],"summary":"B"}"#, "not both"),
    ] {
        let reply = talk(&dir, call("adsertio_claim", args, 1).as_bytes());
        assert!(is_error(&reply[0]), "{args}: {}", reply[0]);
        assert!(reply[0].contains(want), "{args}: {}", reply[0]);
        assert!(claim_files(&dir).is_empty(), "{args} wrote a claim");
    }
    let reply = talk(
        &dir,
        call(
            "adsertio_claim",
            r#"{"claims":[{"summary":"Fine"},{"summary":"Bad","evidence":["nope:x"]}]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(reply[0].contains("Nothing was written"), "{}", reply[0]);
}

#[test]
fn a_batch_over_fifty_items_is_refused() {
    let dir = project("v130-batch-51");
    init(&dir);
    let items: Vec<String> = (0..51)
        .map(|n| format!(r#"{{"summary":"Claim {n}"}}"#))
        .collect();
    let args = format!(r#"{{"claims":[{}]}}"#, items.join(","));
    let reply = talk(&dir, call("adsertio_claim", &args, 1).as_bytes());
    assert!(is_error(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains("more than 50"), "{}", reply[0]);
    assert!(claim_files(&dir).is_empty());
}

#[test]
fn a_duplicate_id_in_the_batch_writes_nothing() {
    let dir = project("v130-batch-dup");
    init(&dir);
    let args = r#"{"claims":[{"summary":"One","id":"same"},{"summary":"Two","id":"other"},{"summary":"Three","id":"same"}]}"#;
    let reply = talk(&dir, call("adsertio_claim", args, 1).as_bytes());
    assert!(is_error(&reply[0]), "{}", reply[0]);
    assert!(
        reply[0].contains("claims item 3") && reply[0].contains("also claims item 1"),
        "{}",
        reply[0]
    );
    assert!(claim_files(&dir).is_empty());
}

#[test]
fn an_existing_id_writes_nothing_and_is_never_overwritten() {
    let dir = project("v130-batch-exists");
    init(&dir);
    let out = cli(&dir, &["claim", "--summary", "Original", "--id", "taken"]);
    assert!(out.status.success(), "{}", text(&out));
    let before = fs::read_to_string(dir.join(".adsertio/claims/taken.md")).unwrap();
    let args = r#"{"claims":[{"summary":"New","id":"fresh"},{"summary":"Clash","id":"taken"}]}"#;
    let reply = talk(&dir, call("adsertio_claim", args, 1).as_bytes());
    assert!(is_error(&reply[0]), "{}", reply[0]);
    assert!(
        reply[0].contains("claims item 2") && reply[0].contains("already exists"),
        "{}",
        reply[0]
    );
    assert_eq!(claim_files(&dir), ["taken.md"]);
    let after = fs::read_to_string(dir.join(".adsertio/claims/taken.md")).unwrap();
    assert_eq!(before, after);
    // An item without an id steps past the taken one, as a single claim does.
    let args = r#"{"claims":[{"summary":"Taken"}]}"#;
    let reply = talk(&dir, call("adsertio_claim", args, 1).as_bytes());
    assert!(reply[0].contains("taken-2"), "{}", reply[0]);
}

#[test]
fn the_single_claim_form_still_works() {
    let dir = project("v130-single");
    init(&dir);
    let reply = talk(
        &dir,
        call("adsertio_claim", r#"{"summary":"Alone","id":"alone"}"#, 1).as_bytes(),
    );
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains(r#"\"id\":\"alone\""#), "{}", reply[0]);
    let reply = talk(&dir, call("adsertio_claim", "{}", 1).as_bytes());
    assert!(is_error(&reply[0]), "{}", reply[0]);
    assert!(
        reply[0].contains("summary") && reply[0].contains("claims"),
        "{}",
        reply[0]
    );
}

#[test]
fn the_cli_takes_claims_as_json_or_a_file() {
    let dir = project("v130-cli");
    init(&dir);
    let out = cli(
        &dir,
        &[
            "claim",
            "--claims",
            r#"[{"summary":"From flag","id":"flag"}]"#,
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("\"ids\":[\"flag\"]"), "{}", text(&out));

    // A file, with the byte order mark Windows PowerShell 5 writes.
    let file = dir.join("claims.json");
    fs::write(
        &file,
        "\u{feff}[{\"summary\":\"From file\",\"id\":\"file\",\"evidence\":[\"chisel:.chisel/store#test:t1\"]}]",
    )
    .unwrap();
    let out = cli(&dir, &["claim", "--claims-file", file.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("\"file\""), "{}", text(&out));
    assert_eq!(claim_files(&dir), ["file.md", "flag.md"]);

    // Usage errors: both forms, bad JSON, not an array, a missing file.
    let path = file.to_str().unwrap();
    for args in [
        vec!["claim", "--claims", "[]", "--claims-file", path],
        vec!["claim", "--claims", "[{"],
        vec!["claim", "--claims", r#"{"summary":"x"}"#],
        vec!["claim", "--claims-file", "no-such-file.json"],
        vec!["claim", "--evidence-file", path],
    ] {
        let out = cli(&dir, &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
    }
    // A bad item is a tool error (exit 1), and writes nothing.
    let out = cli(
        &dir,
        &[
            "claim",
            "--claims",
            r#"[{"summary":"ok","id":"x1"},{"summary":""}]"#,
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("claims item 2"), "{}", text(&out));
    assert_eq!(claim_files(&dir), ["file.md", "flag.md"]);
}

// Batch attach.

#[test]
fn attach_items_updates_several_claims_or_none() {
    let dir = project("v130-attach");
    init(&dir);
    let reply = talk(
        &dir,
        call(
            "adsertio_claim",
            r#"{"claims":[{"summary":"A","id":"a"},{"summary":"B","id":"b","evidence":["chisel:.chisel/store#test:t1"]}]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    let read =
        |id: &str| fs::read_to_string(dir.join(format!(".adsertio/claims/{id}.md"))).unwrap();
    let (a0, b0) = (read("a"), read("b"));

    for (args, want) in [
        (
            r#"{"items":[{"id":"a","evidence":["trammel:.trammel/store#plan-1"]},{"id":"nope","evidence":["trammel:.trammel/store#plan-1"]}]}"#,
            "attach item 2: no claim with id",
        ),
        (
            r#"{"items":[{"id":"a","evidence":["trammel:.trammel/store#plan-1"]},{"id":"b","evidence":["chisel:.chisel/store#test:t1"]}]}"#,
            "already on claim",
        ),
        (
            r#"{"items":[{"id":"a","evidence":["trammel:.trammel/store#plan-1"]},{"id":"a","evidence":["trammel:.trammel/store#plan-2"]}]}"#,
            "also attach item 1",
        ),
        (
            r#"{"items":[{"id":"a","evidence":["bad"]}]}"#,
            "attach item 1",
        ),
        (
            r#"{"items":[{"id":"a","evidence":[]}]}"#,
            "evidence is empty",
        ),
        (
            r#"{"items":[{"id":"a"}]}"#,
            "missing required argument: evidence",
        ),
        (
            r#"{"items":[{"id":"a","evidence":["trammel:.trammel/store#plan-1"]}],"id":"a"}"#,
            "not both",
        ),
        (r#"{"items":[]}"#, "items is empty"),
    ] {
        let reply = talk(&dir, call("adsertio_attach", args, 1).as_bytes());
        assert!(is_error(&reply[0]), "{args}: {}", reply[0]);
        assert!(reply[0].contains(want), "{args}: {}", reply[0]);
        assert_eq!(read("a"), a0, "{args} changed a");
        assert_eq!(read("b"), b0, "{args} changed b");
    }

    let args = r#"{"items":[{"id":"a","evidence":["trammel:.trammel/store#plan-1","trammel:.trammel/store#plan-2"]},{"id":"b","evidence":["probatio:.probatio/run"]}]}"#;
    let reply = talk(&dir, call("adsertio_attach", args, 1).as_bytes());
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    let body = json::parse(&result_text(&reply[0])).unwrap();
    let claims = body.get("claims").unwrap().as_array().unwrap();
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].get("evidence").and_then(Value::as_u64), Some(2));
    assert_eq!(claims[1].get("evidence").and_then(Value::as_u64), Some(2));
    assert!(read("a").contains("plan-2"));
    assert!(read("b").contains("probatio:.probatio/run"));
    assert!(fs::read_dir(dir.join(".adsertio/claims"))
        .unwrap()
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().contains(".lock")));

    // The CLI form, and the single form.
    let out = cli(
        &dir,
        &[
            "attach",
            "--items",
            r#"[{"id":"a","evidence":["trammel:.trammel/store#constraint-1"]}]"#,
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let out = cli(
        &dir,
        &[
            "attach",
            "--id",
            "b",
            "--evidence",
            "trammel:.trammel/store#failure-1",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("\"added\""), "{}", text(&out));
    let reply = talk(&dir, call("adsertio_attach", r#"{"id":"a"}"#, 1).as_bytes());
    assert!(is_error(&reply[0]), "{}", reply[0]);
}

// The hook.

fn json_part(snippet: &str) -> String {
    snippet
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn no_shell_syntax(command: &str) {
    for bad in ["&&", "||", "~", "'", "\"", "$", ";", "|", ">", "<", "%"] {
        assert!(!command.contains(bad), "{command} has {bad}");
    }
}

#[test]
fn the_claude_hook_is_valid_json_with_an_exec_command() {
    let dir = project("v130-hook");
    let out = cli(&dir, &["hook", "--host", "claude"]);
    assert!(out.status.success(), "{}", text(&out));
    let snippet = String::from_utf8(out.stdout).unwrap();
    assert!(snippet.contains(".claude/settings.json"), "{snippet}");
    let doc = json::parse(&json_part(&snippet)).expect("the claude snippet is JSON");
    let start = &doc
        .get("hooks")
        .unwrap()
        .get("SessionStart")
        .unwrap()
        .as_array()
        .unwrap()[0];
    let matcher = start.get("matcher").unwrap().as_str().unwrap();
    assert!(
        matcher.contains("compact") && matcher.contains("resume"),
        "{matcher}"
    );
    let hook = &start.get("hooks").unwrap().as_array().unwrap()[0];
    assert_eq!(hook.get("type").and_then(Value::as_str), Some("command"));
    assert_eq!(
        hook.get("command").and_then(Value::as_str),
        Some("adsertio")
    );
    let args: Vec<&str> = hook
        .get("args")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(
        args,
        [
            "view",
            "--compact",
            "--states",
            "unverified",
            "--states",
            "refuted",
            "--limit",
            "50",
            "--if-store"
        ]
    );
    for arg in &args {
        no_shell_syntax(arg);
    }
    let sub = &doc
        .get("hooks")
        .unwrap()
        .get("SubagentStart")
        .unwrap()
        .as_array()
        .unwrap()[0];
    let sub_args = sub.get("hooks").unwrap().as_array().unwrap()[0]
        .get("args")
        .unwrap()
        .to_json();
    assert!(sub_args.contains("claude-subagent"), "{sub_args}");
}

#[test]
fn the_cursor_and_generic_hooks_name_the_view() {
    let dir = project("v130-hook-other");
    let out = cli(&dir, &["hook", "--host", "cursor"]);
    assert!(out.status.success(), "{}", text(&out));
    let snippet = String::from_utf8(out.stdout).unwrap();
    assert!(snippet.contains(".cursor/hooks.json"), "{snippet}");
    assert!(
        snippet.contains("preCompact cannot put text back"),
        "{snippet}"
    );
    let doc = json::parse(&json_part(&snippet)).expect("the cursor snippet is JSON");
    let command = doc
        .get("hooks")
        .unwrap()
        .get("sessionStart")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .get("command")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        command,
        "adsertio view --compact --states unverified --states refuted --limit 50 --if-store --wrap cursor"
    );
    no_shell_syntax(&command);

    let generic = text(&cli(&dir, &["hook"]));
    assert!(
        generic.contains(
            "\nadsertio view --compact --states unverified --states refuted --limit 50 --if-store\n"
        ),
        "{generic}"
    );
    assert!(generic.contains("after compaction"), "{generic}");
    let generic2 = text(&cli(&dir, &["hook", "--host", "generic"]));
    assert_eq!(generic, generic2);

    for args in [
        vec!["hook", "--host", "vim"],
        vec!["hook", "--colour", "red"],
        vec!["hook", "--host"],
        vec!["hook", "--host", "claude", "--host", "cursor"],
    ] {
        assert_eq!(cli(&dir, &args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn the_hook_view_lists_only_open_claims_and_wraps_for_hosts() {
    let dir = project("v130-hook-view");
    init(&dir);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(
        dir.join(".trammel/store"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true}}],"constraints":[],"failures":[]}"#,
    )
    .unwrap();
    let out = cli(
        &dir,
        &[
            "claim",
            "--claims",
            r#"[{"summary":"Done","id":"done","evidence":["trammel:.trammel/store#plan-1/step-1"]},{"summary":"Open","id":"open"}]"#,
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let out = cli(
        &dir,
        &[
            "view",
            "--compact",
            "--states",
            "unverified",
            "--states",
            "refuted",
        ],
    );
    let body = text(&out);
    assert!(out.status.success(), "{body}");
    assert!(
        body.contains("\"open\"") && !body.contains("\"done\""),
        "{body}"
    );

    for (host, key) in [
        ("cursor", "additional_context"),
        ("claude-subagent", "additionalContext"),
    ] {
        let out = cli(
            &dir,
            &[
                "view",
                "--compact",
                "--states",
                "unverified",
                "--wrap",
                host,
            ],
        );
        assert!(out.status.success(), "{}", text(&out));
        let doc = json::parse(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
        let context = if host == "cursor" {
            doc.get(key).unwrap().as_str().unwrap().to_string()
        } else {
            let inner = doc.get("hookSpecificOutput").unwrap();
            assert_eq!(
                inner.get("hookEventName").and_then(Value::as_str),
                Some("SubagentStart")
            );
            inner.get(key).unwrap().as_str().unwrap().to_string()
        };
        assert!(context.contains("\"open\""), "{context}");
        assert!(
            context.contains("do not repeat one from memory"),
            "{context}"
        );
    }
    assert_eq!(cli(&dir, &["view", "--wrap", "vim"]).status.code(), Some(2));
    assert_eq!(cli(&dir, &["view", "--wrap"]).status.code(), Some(2));
}

// Doctor.

fn doctor(dir: &Path, args: &[&str]) -> (i32, Value) {
    let mut all = vec!["doctor"];
    all.extend_from_slice(args);
    let out = cli(dir, &all);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let doc = json::parse(stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", text(&out)));
    (out.status.code().unwrap(), doc)
}

fn check<'a>(doc: &'a Value, name: &str) -> &'a Value {
    doc.get("checks")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c.get("name").and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no {name} check in {}", doc.to_json()))
}

fn ok(check: &Value) -> bool {
    check.get("ok").and_then(Value::as_bool).unwrap()
}

#[test]
fn doctor_passes_on_a_healthy_store() {
    let dir = repo("v130-doctor-ok");
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(
        dir.join(".chisel/store"),
        r#"{"magic":"chisel-store","version":1,"test_results":[],"test_edges":[]}"#,
    )
    .unwrap();
    let out = cli(&dir, &["claim", "--summary", "Healthy"]);
    assert!(out.status.success(), "{}", text(&out));
    let (code, doc) = doctor(&dir, &[]);
    assert_eq!(code, 0, "{}", doc.to_json());
    assert_eq!(doc.get("ok").and_then(Value::as_bool), Some(true));
    for name in [
        "version",
        "store",
        "store_writable",
        "claims",
        "leftover_files",
        "git",
        "trammel_store",
        "chisel_store",
        "probatio",
        "binaries",
    ] {
        let c = check(&doc, name);
        assert!(ok(c), "{}", c.to_json());
        assert!(c.get("detail").and_then(Value::as_str).is_some());
        assert_eq!(c.get("fix"), Some(&Value::Null));
    }
    let version = check(&doc, "version")
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(version.contains(env!("CARGO_PKG_VERSION")), "{version}");
    assert!(version.contains(std::env::consts::OS), "{version}");
    let claims = check(&doc, "claims")
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(claims.starts_with("1 claims"), "{claims}");
    let chisel = check(&doc, "chisel_store")
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(chisel.contains("can read it"), "{chisel}");
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let gitd = check(&doc, "git").get("detail").unwrap().as_str().unwrap();
    assert!(gitd.contains(&head), "{gitd}");
    // Read only: no probe or temp file is left, and the claim is untouched.
    assert_eq!(claim_files(&dir), ["healthy.md"]);
    assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));
    if cfg!(windows) {
        assert!(ok(check(&doc, "path_length")));
    }

    // The MCP tool returns the same report.
    let reply = talk(&dir, call("adsertio_doctor", "{}", 1).as_bytes());
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains(r#"\"ok\":true"#), "{}", reply[0]);
    let reply = talk(
        &dir,
        call("adsertio_doctor", r#"{"repair":true}"#, 1).as_bytes(),
    );
    assert!(is_error(&reply[0]), "{}", reply[0]);
}

/// A pid that is not running: a child that has already exited.
fn dead_pid() -> u32 {
    let mut child = Command::new(CLI).arg("--version").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

#[test]
fn doctor_reports_planted_problems_with_a_fix() {
    let dir = repo("v130-doctor-bad");
    let claims = dir.join(".adsertio/claims");
    fs::write(claims.join("broken.md"), "no frontmatter\n").unwrap();
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), "{not json").unwrap();
    let (code, doc) = doctor(&dir, &[]);
    assert_eq!(code, 1, "{}", doc.to_json());
    assert_eq!(doc.get("ok").and_then(Value::as_bool), Some(false));
    for name in ["claims", "trammel_store"] {
        let c = check(&doc, name);
        assert!(!ok(c), "{}", c.to_json());
        let fix = c.get("fix").and_then(Value::as_str).unwrap();
        assert!(fix.len() > 20, "{fix}");
    }
    assert!(check(&doc, "claims").to_json().contains("broken"));
    // Doctor never repairs or removes a damaged claim.
    assert!(claims.join("broken.md").exists());
    fs::remove_file(claims.join("broken.md")).unwrap();
    fs::remove_file(dir.join(".trammel/store")).unwrap();

    // Leftovers from a writer that is gone: reported, then cleared by --fix.
    let pid = dead_pid();
    let temp = format!(".x.md.tmp-{pid}-1");
    fs::write(claims.join(&temp), "partial").unwrap();
    fs::write(claims.join(".x.md.lock"), format!("{pid} 0 0-0\n")).unwrap();
    let (code, doc) = doctor(&dir, &[]);
    assert_eq!(code, 1, "{}", doc.to_json());
    let left = check(&doc, "leftover_files");
    assert!(!ok(left), "{}", left.to_json());
    assert!(left.to_json().contains(&temp), "{}", left.to_json());
    assert!(left.get("fix").unwrap().as_str().unwrap().contains("--fix"));
    assert!(
        claims.join(&temp).exists(),
        "doctor without --fix removed a file"
    );
    assert!(claims.join(".x.md.lock").exists());

    let (code, doc) = doctor(&dir, &["--fix"]);
    assert_eq!(code, 0, "{}", doc.to_json());
    let fixed = doc.get("fixed").unwrap().to_json();
    assert!(
        fixed.contains(&temp) && fixed.contains(".x.md.lock"),
        "{fixed}"
    );
    assert!(!claims.join(&temp).exists());
    assert!(!claims.join(".x.md.lock").exists());

    // No store: the store check fails and says how to make one.
    let bare = project("v130-doctor-bare");
    let (code, doc) = doctor(&bare, &[]);
    assert_eq!(code, 1);
    let store = check(&doc, "store");
    assert!(!ok(store));
    assert!(store
        .get("fix")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("adsertio init"));
}

#[test]
fn doctor_reports_a_folder_that_is_not_a_repository() {
    let dir = project("v130-doctor-nogit");
    init(&dir);
    let (code, doc) = doctor(&dir, &[]);
    // A temp folder may sit inside some other repository; only check the
    // failure when git says there is none.
    let inside = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !inside {
        assert_eq!(code, 1);
        let g = check(&doc, "git");
        assert!(!ok(g), "{}", g.to_json());
        assert!(g.get("fix").unwrap().as_str().unwrap().contains("git"));
    }
}

// The stale-after-commit hint.

#[test]
fn a_pass_on_an_older_commit_says_to_rerun_at_head() {
    let dir = repo("v130-stale-hint");
    let old = git(&dir, &["rev-parse", "HEAD"]);
    fs::write(dir.join("tracked.txt"), "y").unwrap();
    git(&dir, &["commit", "-am", "second"]);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(
        dir.join(".chisel/store"),
        format!(
            r#"{{"magic":"chisel-store","version":1,"test_results":[{{"test_id":"t1","passed":true,"recorded_at":4102444800}}],"test_passes":{{"t1":{{"commit":"{old}","at":4102444800}}}},"test_edges":[]}}"#
        ),
    )
    .unwrap();
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(
        dir.join(".trammel/store"),
        format!(
            r#"{{"schema_version":1,"plans":[{{"id":1}}],"steps":[{{"id":1,"plan_id":1,"verification":{{"passed":true,"at":4102444800,"commit":"{old}"}}}}],"constraints":[],"failures":[]}}"#
        ),
    )
    .unwrap();
    let out = cli(
        &dir,
        &[
            "claim",
            "--claims",
            r#"[{"summary":"Chisel","id":"c","evidence":["chisel:.chisel/store#test:t1"]},{"summary":"Trammel","id":"t","evidence":["trammel:.trammel/store#plan-1/step-1"]}]"#,
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let out = cli(&dir, &["view"]);
    let doc = json::parse(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let claims = doc.get("claims").unwrap().as_array().unwrap();
    for claim in claims {
        let ev = &claim.get("evidence").unwrap().as_array().unwrap()[0];
        assert_eq!(ev.get("state").and_then(Value::as_str), Some("stale"));
        let detail = ev.get("detail").unwrap().as_str().unwrap();
        assert!(detail.contains("is not HEAD"), "{detail}");
        if claim.get("id").and_then(Value::as_str) == Some("c") {
            assert!(
                detail.ends_with(
                    "rerun the tests at HEAD (for example chisel run -- <test command>)"
                ),
                "{detail}"
            );
        } else {
            assert!(detail.contains("rerun the tests at HEAD"), "{detail}");
        }
    }
    // An old timestamp is a different stale, without the commit hint.
    fs::write(
        dir.join(".chisel/store"),
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"t1","passed":true,"recorded_at":1000}],"test_edges":[]}"#,
    )
    .unwrap();
    let body = text(&cli(&dir, &["view", "--id", "c"]));
    assert!(body.contains("older than the claim"), "{body}");
    assert!(!body.contains("rerun the tests at HEAD"), "{body}");
}

#[test]
fn the_tools_tell_an_agent_to_batch() {
    let dir = project("v130-describe");
    let reply = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    );
    assert!(
        reply[0].contains("Record all claims in one call"),
        "{}",
        reply[0]
    );
    assert!(reply[0].contains("adsertio_doctor"), "{}", reply[0]);
    assert!(reply[0].contains(r#""maxItems":50"#), "{}", reply[0]);
    let guide = talk(&dir, call("adsertio_guide", "{}", 1).as_bytes());
    assert!(
        guide[0].contains("Record all claims in one call"),
        "{}",
        guide[0]
    );
}

#[test]
fn readme_and_spec_describe_1_3_0() {
    let readme = include_str!("../README.md");
    let spec = include_str!("../SPEC.md");
    let log = include_str!("../CHANGELOG.md");
    for text in [readme, spec] {
        assert!(text.contains("adsertio hook"));
        assert!(text.contains("adsertio doctor") || text.contains("adsertio_doctor"));
        assert!(text.contains("claims"));
    }
    assert!(log.contains("## 1.3.0"));
    assert!(!log.contains("## Unreleased"));
    for text in [readme, spec, log] {
        assert!(text.is_ascii(), "docs are plain ASCII");
    }
}

// A hook set for every project must stay quiet where there is no store.
#[test]
fn view_if_store_is_silent_without_a_store() {
    let dir = project("v130-if-store");
    let out = std::process::Command::new(CLI)
        .args(["view", "--compact", "--if-store"])
        .current_dir(&dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    // Without the flag it is still an error.
    let out = std::process::Command::new(CLI)
        .args(["view", "--compact"])
        .current_dir(&dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    // With a store it prints the view as usual.
    init(&dir);
    let out = std::process::Command::new(CLI)
        .args(["view", "--compact", "--if-store"])
        .current_dir(&dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"claims\""));
}
