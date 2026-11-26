//! 1.5.0: `relatum:<store>#<note-id>`, a Relatum note as evidence. Every row
//! of the state table, the claim and the view through the CLI and MCP, mixed
//! claims, hostile paths, and that a Relatum pass is never stale.
mod common;
use adsertio::json::{self, Value};
use adsertio::verdict::State;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const TRAMMEL: &str = include_str!("fixtures/trammel/store.json");

fn note(kind: &str, extra: &str) -> String {
    format!("---\nkind: {kind}\nsummary: A rule.\nauthor: parent\nupdated: 2020-01-01\n{extra}---\n\nThe body.\n")
}

fn put(dir: &Path, id: &str, text: &str) {
    let path = dir.join(".swarmnotes").join(format!("{id}.md"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A project with a store: one note in each status.
fn notes(name: &str) -> PathBuf {
    let dir = project(name);
    put(&dir, "net/retry", &note("decision", ""));
    put(
        &dir,
        "_shared/style",
        &note("convention", "status: active\n"),
    );
    put(
        &dir,
        "net/old",
        &note(
            "constraint",
            "status: superseded\nsuperseded_by: net/retry\n",
        ),
    );
    put(&dir, "net/wip", &note("constraint", "status: draft\n"));
    dir
}

fn at(dir: &Path, pointer: &str) -> adsertio::verdict::Evidence {
    adsertio::verdict::read_evidence(dir, pointer)
}

fn state(dir: &Path, pointer: &str, want: State) -> String {
    let found = at(dir, pointer);
    assert_eq!(found.state, want, "{pointer}: {}", found.detail);
    found.detail
}

fn listing(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(d: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(d).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(&p, out);
            } else {
                out.push((p.clone(), fs::read(&p).unwrap()));
            }
        }
    }
    walk(dir, &mut out);
    out
}

fn cli(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .env_remove("SWARMNOTES_DIR")
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

fn view_one(dir: &Path, id: &str) -> Value {
    let (ok, text) = cli(dir, &["view", "--id", id]);
    assert!(ok, "{text}");
    let doc = json::parse(&text).unwrap();
    doc.get("claims").unwrap().as_array().unwrap()[0].clone()
}

fn verdict_and_states(claim: &Value) -> (String, Vec<String>) {
    let states = claim
        .get("evidence")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.get("state").unwrap().as_str().unwrap().to_string())
        .collect();
    (
        claim.get("verdict").unwrap().as_str().unwrap().to_string(),
        states,
    )
}

#[test]
fn every_state_in_the_table() {
    let dir = notes("relatum-states");
    let before = listing(&dir.join(".swarmnotes"));

    let pass = state(&dir, "relatum:.swarmnotes#net/retry", State::Pass);
    assert!(
        pass.contains("net/retry") && pass.contains("active"),
        "{pass}"
    );
    state(&dir, "relatum:.swarmnotes#_shared/style", State::Pass);
    state(&dir, "relatum:.swarmnotes/#net/retry", State::Pass);
    state(&dir, "relatum:./.swarmnotes#net/retry", State::Pass);

    let fail = state(&dir, "relatum:.swarmnotes#net/old", State::Fail);
    assert!(fail.contains("superseded by net/retry"), "{fail}");
    assert!(
        fail.contains("point the claim at relatum:.swarmnotes#net/retry"),
        "{fail}"
    );

    let draft = state(&dir, "relatum:.swarmnotes#net/wip", State::Inconclusive);
    assert!(draft.contains("draft"), "{draft}");

    let gone = state(&dir, "relatum:.swarmnotes#net/nope", State::Missing);
    assert!(gone.contains("no note net/nope"), "{gone}");
    let no_store = state(&dir, "relatum:.notes#net/retry", State::Missing);
    assert!(no_store.contains("no Relatum store"), "{no_store}");
    let bare_dir = project("relatum-no-store");
    state(&bare_dir, "relatum:.swarmnotes#net/retry", State::Missing);

    let bare = state(&dir, "relatum:.swarmnotes", State::Unreadable);
    assert!(bare.contains("note id is required"), "{bare}");
    state(&dir, "relatum:.swarmnotes#", State::Unreadable);
    // No selector is unreadable whether or not the store is there.
    state(&bare_dir, "relatum:.swarmnotes", State::Unreadable);
    for id in [
        "Net/Retry",
        "net/",
        "/net",
        "net//retry",
        "net/retry.md",
        "-a",
        "_",
    ] {
        let bad = state(
            &dir,
            &format!("relatum:.swarmnotes#{id}"),
            State::Unreadable,
        );
        assert!(bad.contains("not a Relatum note id"), "{id}: {bad}");
    }

    for (text, why) in [
        ("kind: decision\nsummary: s\n", "does not start"),
        ("---\nkind: decision\nsummary: s\n", "never closed"),
        (
            "---\nkind decision\nsummary: s\n---\n\nb\n",
            "not key: value",
        ),
        ("---\nsummary: s\n---\n\nb\n", "kind is missing"),
        ("---\nkind: decision\n---\n\nb\n", "summary is missing"),
        (
            "---\nkind: decision\nsummary: s\nstatus: retired\n---\n\nb\n",
            "is not active, draft or superseded",
        ),
        (
            "---\nkind: decision\nsummary: s\nstatus: superseded\n---\n\nb\n",
            "superseded_by is missing",
        ),
    ] {
        put(&dir, "bad/note", text);
        let damaged = state(&dir, "relatum:.swarmnotes#bad/note", State::Unreadable);
        assert!(
            damaged.contains("note bad/note is damaged") && damaged.contains(why),
            "{text:?}: {damaged}"
        );
    }
    fs::remove_file(dir.join(".swarmnotes/bad/note.md")).unwrap();
    fs::remove_dir(dir.join(".swarmnotes/bad")).unwrap();

    // The store must be a folder.
    fs::write(dir.join("notes.txt"), note("decision", "")).unwrap();
    let file = state(&dir, "relatum:notes.txt#net/retry", State::Unreadable);
    assert!(file.contains("not a folder"), "{file}");
    fs::remove_file(dir.join("notes.txt")).unwrap();

    // Reading wrote nothing into the store and took no lock there.
    assert_eq!(listing(&dir.join(".swarmnotes")), before);
}

#[test]
fn notes_written_on_windows_read_the_same() {
    let dir = project("relatum-crlf");
    put(&dir, "a/crlf", &note("decision", "").replace('\n', "\r\n"));
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend(note("decision", "status: draft\n").as_bytes());
    put(&dir, "a/bom", std::str::from_utf8(&bom).unwrap());
    state(&dir, "relatum:.swarmnotes#a/crlf", State::Pass);
    state(&dir, "relatum:.swarmnotes#a/bom", State::Inconclusive);
}

#[test]
fn the_supersede_chain_is_not_followed() {
    let dir = notes("relatum-chain");
    // net/old points at net/retry; once net/retry is itself superseded, a
    // pointer at net/old still names net/retry, and net/retry fails.
    put(
        &dir,
        "net/retry",
        &note("decision", "status: superseded\nsuperseded_by: net/newer\n"),
    );
    let old = state(&dir, "relatum:.swarmnotes#net/old", State::Fail);
    assert!(old.contains("superseded by net/retry"), "{old}");
    assert!(!old.contains("net/newer"), "{old}");
    state(&dir, "relatum:.swarmnotes#net/retry", State::Fail);
    // The replacement need not exist for the superseded note to fail.
    state(&dir, "relatum:.swarmnotes#net/newer", State::Missing);
}

#[test]
fn hostile_paths_are_not_proof() {
    let dir = notes("relatum-hostile");
    for pointer in [
        "relatum:../x#a",
        "relatum:.swarmnotes/../.swarmnotes#net/retry",
        "relatum:/etc#passwd",
        "relatum:C:/x#a",
        r"relatum:.swarmnotes\net#retry",
        "relatum:.swarmnotes#a,b",
        "relatum:.swarmnotes#a b",
        "relatum:.swarmnotes#net/../net/retry",
    ] {
        state(&dir, pointer, State::Unreadable);
    }
    let long = format!("relatum:.swarmnotes#{}", "a".repeat(290));
    let over = state(&dir, &long, State::Unreadable);
    assert!(over.contains("longer than 300 characters"), "{over}");

    // A folder named like a note is not a note.
    fs::create_dir_all(dir.join(".swarmnotes/net/folder.md")).unwrap();
    let folder = state(&dir, "relatum:.swarmnotes#net/folder", State::Unreadable);
    assert!(folder.contains("is a folder"), "{folder}");
    // A scope folder with no note file is just no note.
    state(&dir, "relatum:.swarmnotes#net", State::Missing);

    // The claim itself refuses a path that leaves the project.
    init(&dir);
    for pointer in ["relatum:../x#a", "relatum:/etc#a", long.as_str()] {
        let (ok, text) = cli(&dir, &["claim", "--summary", "Bad", "--evidence", pointer]);
        assert!(!ok, "{pointer}: {text}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_out_are_refused() {
    use std::os::unix::fs::symlink;
    let dir = notes("relatum-link");
    let outside = project("relatum-link-out");
    put(&outside, "x", &note("decision", ""));
    // A store that is a symlink out of the project.
    symlink(outside.join(".swarmnotes"), dir.join("evil")).unwrap();
    let store = state(&dir, "relatum:evil#x", State::Unreadable);
    assert!(store.contains("outside the project"), "{store}");
    // A note that is a symlink out of the project.
    symlink(
        outside.join(".swarmnotes/x.md"),
        dir.join(".swarmnotes/net/linked.md"),
    )
    .unwrap();
    let linked = state(&dir, "relatum:.swarmnotes#net/linked", State::Unreadable);
    assert!(linked.contains("outside the project"), "{linked}");
    // A note that is a symlink out of the store, inside the project.
    fs::write(dir.join("loose.md"), note("decision", "")).unwrap();
    symlink(dir.join("loose.md"), dir.join(".swarmnotes/net/loose.md")).unwrap();
    let loose = state(&dir, "relatum:.swarmnotes#net/loose", State::Unreadable);
    assert!(loose.contains("outside the store"), "{loose}");
    // A symlinked scope folder out of the project.
    symlink(outside.join(".swarmnotes"), dir.join(".swarmnotes/far")).unwrap();
    state(&dir, "relatum:.swarmnotes#far/x", State::Unreadable);
    // A symlink inside the store, to a note in the store, still reads.
    symlink(
        dir.join(".swarmnotes/net/retry.md"),
        dir.join(".swarmnotes/net/alias.md"),
    )
    .unwrap();
    state(&dir, "relatum:.swarmnotes#net/alias", State::Pass);
}

#[test]
fn bytes_that_are_not_a_note() {
    let dir = notes("relatum-bytes");
    let mut bytes = note("decision", "").into_bytes();
    bytes.extend([0xff, 0xfe]);
    fs::write(dir.join(".swarmnotes/net/binary.md"), bytes).unwrap();
    let binary = state(&dir, "relatum:.swarmnotes#net/binary", State::Unreadable);
    assert!(binary.contains("not valid UTF-8"), "{binary}");

    // Over the evidence read limit: refused from its size, before a read.
    let big = dir.join(".swarmnotes/net/big.md");
    let file = fs::File::create(&big).unwrap();
    file.set_len(adsertio::verdict::MAX_EVIDENCE_BYTES + 1)
        .unwrap();
    drop(file);
    let over = state(&dir, "relatum:.swarmnotes#net/big", State::Unreadable);
    assert!(over.contains("larger than"), "{over}");
    fs::remove_file(&big).unwrap();
}

#[test]
fn claim_and_view_through_the_cli() {
    let dir = notes("relatum-cli");
    init(&dir);
    let (ok, text) = cli(
        &dir,
        &[
            "claim",
            "--summary",
            "Retries use backoff",
            "--id",
            "retry",
            "--evidence",
            "relatum:.swarmnotes#net/retry",
        ],
    );
    assert!(ok, "{text}");
    let claim = view_one(&dir, "retry");
    assert_eq!(
        verdict_and_states(&claim),
        ("verified".to_string(), vec!["pass".to_string()])
    );
    let (ok, text) = cli(&dir, &["check"]);
    assert!(ok, "{text}");

    // Relatum supersedes the rule: the same claim is now refuted, and the
    // detail says where to point it.
    put(
        &dir,
        "net/retry",
        &note(
            "decision",
            "status: superseded\nsuperseded_by: net/retry-v2\n",
        ),
    );
    let claim = view_one(&dir, "retry");
    assert_eq!(
        verdict_and_states(&claim),
        ("refuted".to_string(), vec!["fail".to_string()])
    );
    let detail = claim.get("evidence").unwrap().as_array().unwrap()[0]
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(detail.contains("net/retry-v2"), "{detail}");

    // An unknown tool is refused, and the error lists relatum.
    let (ok, text) = cli(
        &dir,
        &["claim", "--summary", "No", "--evidence", "notes:a#b"],
    );
    assert!(!ok && text.contains("relatum"), "{text}");
    let (_, help) = cli(&dir, &["--help"]);
    assert!(help.contains("relatum"), "{help}");
}

#[test]
fn claim_and_view_through_mcp() {
    let dir = notes("relatum-mcp");
    init(&dir);
    let mut input = call(
        "adsertio_claim",
        r#"{"claims":[{"summary":"Style rule holds","id":"style","evidence":["relatum:.swarmnotes#_shared/style"]},{"summary":"Old rule holds","id":"old","evidence":["relatum:.swarmnotes#net/old"]},{"summary":"Draft rule holds","id":"wip","evidence":["relatum:.swarmnotes#net/wip"]}]}"#,
        1,
    );
    for (n, id) in ["style", "old", "wip"].iter().enumerate() {
        input += &call(
            "adsertio_view",
            &format!(r#"{{"id":"{id}"}}"#),
            2 + n as u32,
        );
    }
    input += &call("adsertio_guide", "{}", 9);
    let replies = talk(&dir, input.as_bytes());
    assert!(is_ok(&replies[0]), "{}", replies[0]);
    for (reply, verdict, state) in [
        (&replies[1], "verified", "pass"),
        (&replies[2], "refuted", "fail"),
        (&replies[3], "unverified", "inconclusive"),
    ] {
        assert!(is_ok(reply), "{reply}");
        assert!(
            reply.contains(&format!(r#"\"verdict\":\"{verdict}\""#)),
            "{reply}"
        );
        assert!(
            reply.contains(&format!(r#"\"state\":\"{state}\""#)),
            "{reply}"
        );
    }
    assert!(replies[2].contains("net/retry"), "{}", replies[2]);
    assert!(
        replies[4].contains("relatum:.swarmnotes#<note-id>"),
        "{}",
        replies[4]
    );
}

#[test]
fn mixed_claims_combine_as_usual() {
    let dir = notes("relatum-mixed");
    init(&dir);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), TRAMMEL).unwrap();
    let (ok, text) = cli(
        &dir,
        &[
            "claim",
            "--claims",
            r#"[{"summary":"Trammel passes, the rule was replaced","id":"replaced","evidence":["trammel:.trammel/store#constraint-1","relatum:.swarmnotes#net/old"]},{"summary":"The rule holds, no test result","id":"untested","evidence":["relatum:.swarmnotes#net/retry","chisel:.chisel/store#test:tests/a.rs:t1"]}]"#,
        ],
    );
    assert!(ok, "{text}");
    assert_eq!(
        verdict_and_states(&view_one(&dir, "replaced")),
        (
            "refuted".to_string(),
            vec!["pass".to_string(), "fail".to_string()]
        )
    );
    assert_eq!(
        verdict_and_states(&view_one(&dir, "untested")),
        (
            "unverified".to_string(),
            vec!["pass".to_string(), "missing".to_string()]
        )
    );
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_relatum_pass_is_never_stale() {
    let dir = notes("relatum-fresh");
    init(&dir);
    git(&dir, &["init", "-q"]);
    for (key, value) in [
        ("user.email", "test@example.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        git(&dir, &["config", key, value]);
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "rules"]);
    // A claim made long after the rule was written, with HEAD moved since.
    fs::write(
        dir.join(".adsertio/claims/later.md"),
        "---\nsummary: Written later\nauthor: agent\ncreated: 2099-01-01T00:00:00Z\nevidence: [relatum:.swarmnotes#net/retry]\n---\n",
    )
    .unwrap();
    fs::write(dir.join("code.rs"), "fn main() {}\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "code"]);
    let claim = view_one(&dir, "later");
    assert_eq!(
        verdict_and_states(&claim),
        ("verified".to_string(), vec!["pass".to_string()])
    );
    let detail = claim.get("evidence").unwrap().as_array().unwrap()[0]
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(detail.contains("no timestamp"), "{detail}");

    // Outside a repository too: no git is needed for a rule.
    let loose = notes("relatum-fresh-nogit");
    let mut found = at(&loose, "relatum:.swarmnotes#net/retry");
    adsertio::fresh::apply(&loose, "2099-01-01T00:00:00Z", &mut found);
    assert_eq!(found.state, State::Pass, "{}", found.detail);
}

#[test]
fn doctor_reports_the_relatum_store() {
    fn check(dir: &Path) -> Value {
        let (_, text) = cli(dir, &["doctor"]);
        let doc = json::parse(&text).unwrap();
        doc.get("checks")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c.get("name").and_then(Value::as_str) == Some("relatum"))
            .unwrap_or_else(|| panic!("no relatum check: {text}"))
            .clone()
    }
    let detail = |c: &Value| c.get("detail").unwrap().as_str().unwrap().to_string();
    let passed = |c: &Value| c.get("ok").unwrap().as_bool().unwrap();

    let absent = project("relatum-doctor-absent");
    init(&absent);
    let c = check(&absent);
    assert!(passed(&c) && detail(&c).contains("not present"), "{c:?}");

    let dir = notes("relatum-doctor");
    init(&dir);
    fs::write(dir.join(".swarmnotes/net/.retry.md.1-0-0.tmp"), "partial").unwrap();
    fs::write(dir.join(".swarmnotes/README.txt"), "not a note").unwrap();
    let c = check(&dir);
    assert!(passed(&c), "{c:?}");
    assert!(
        detail(&c).contains("4 note(s)") && detail(&c).contains("2 active, 1 draft, 1 superseded"),
        "{c:?}"
    );

    put(&dir, "net/broken", "no frontmatter\n");
    let c = check(&dir);
    assert!(!passed(&c), "{c:?}");
    assert!(detail(&c).contains("net/broken"), "{c:?}");
    assert!(c.get("fix").unwrap().as_str().is_some(), "{c:?}");

    let file = project("relatum-doctor-file");
    init(&file);
    fs::write(file.join(".swarmnotes"), "x").unwrap();
    let c = check(&file);
    assert!(!passed(&c) && detail(&c).contains("not a folder"), "{c:?}");
}

#[test]
fn a_note_that_names_a_replacement_but_is_not_superseded_is_not_proof() {
    // A supersede reverted by hand (status set back, superseded_by left in)
    // contradicts itself; Relatum's own `get` refuses it, so Adsertio must too.
    let dir = notes("relatum-half-reverted");
    put(
        &dir,
        "net/reverted",
        &note("constraint", "superseded_by: net/retry\n"),
    );
    put(
        &dir,
        "net/draft-with-target",
        &note("constraint", "status: draft\nsuperseded_by: net/retry\n"),
    );
    put(
        &dir,
        "net/blank-target",
        &note("constraint", "superseded_by:\n"),
    );
    for id in ["net/reverted", "net/draft-with-target"] {
        let detail = state(
            &dir,
            &format!("relatum:.swarmnotes#{id}"),
            State::Unreadable,
        );
        assert!(detail.contains("superseded_by is set"), "{detail}");
        assert!(detail.contains("relatum supersede"), "{detail}");
    }
    // A blank value names no replacement and stays an ordinary active rule.
    state(&dir, "relatum:.swarmnotes#net/blank-target", State::Pass);
}
