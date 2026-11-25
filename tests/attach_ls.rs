//! Attaching evidence, and listing claims without reading it.
mod common;
use common::*;
use std::fs;

#[test]
fn attach_adds_a_pointer_and_a_bad_one_writes_nothing() {
    let dir = project("attach");
    init(&dir);
    let made = talk(
        &dir,
        call(
            "adsertio_claim",
            r#"{"summary":"Retry step passed","id":"retry","evidence":["trammel:.trammel/store#plan-1/step-1"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&made[0]), "{}", made[0]);
    let path = dir.join(".adsertio/claims/retry.md");
    let before = fs::read(&path).unwrap();

    let bad = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"retry","evidence":["jest:nope.json","chisel:.chisel/store#test:t"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&bad[0]) && bad[0].contains("not one of"),
        "{}",
        bad[0]
    );
    assert_eq!(fs::read(&path).unwrap(), before);

    let repeated = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"retry","evidence":["chisel:.chisel/store#test:t","chisel:.chisel/store#test:t"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&repeated[0]) && repeated[0].contains("repeated"),
        "{}",
        repeated[0]
    );
    assert_eq!(fs::read(&path).unwrap(), before);

    let added = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"retry","evidence":["chisel:.chisel/store#test:t"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&added[0]), "{}", added[0]);
    let again = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"retry","evidence":["chisel:.chisel/store#test:t"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&again[0]) && again[0].contains("already"),
        "{}",
        again[0]
    );

    let view = talk(
        &dir,
        call("adsertio_view", r#"{"id":"retry"}"#, 1).as_bytes(),
    );
    assert!(is_ok(&view[0]), "{}", view[0]);
    assert!(view[0].contains("plan-1/step-1"), "{}", view[0]);
    assert!(view[0].contains("test:t"), "{}", view[0]);
    assert!(strays(&dir).is_empty());

    let missing = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"no-such","evidence":["chisel:.chisel/store#test:t"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&missing[0]) && missing[0].contains("no claim"),
        "{}",
        missing[0]
    );
    assert!(!dir.join(".adsertio/claims/no-such.md").exists());
}

#[test]
fn ls_ignores_missing_evidence_and_a_corrupt_claim_fails_it() {
    let dir = project("ls");
    init(&dir);
    talk(
        &dir,
        call(
            "adsertio_claim",
            r#"{"summary":"Listed","id":"listed","evidence":["chisel:missing.json#test:t"]}"#,
            1,
        )
        .as_bytes(),
    );
    let listed = talk(&dir, call("adsertio_ls", "{}", 1).as_bytes());
    assert!(is_ok(&listed[0]), "{}", listed[0]);
    assert!(listed[0].contains("listed"), "{}", listed[0]);
    assert!(listed[0].contains(r#"\"pointers\":1"#), "{}", listed[0]);
    assert!(!listed[0].contains("verdict"), "{}", listed[0]);
    assert!(!listed[0].contains("missing"), "{}", listed[0]);

    let path = dir.join(".adsertio/claims/listed.md");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        original.replace("summary: Listed", "verdict: verified\nsummary: Listed"),
    )
    .unwrap();
    let broken = talk(&dir, call("adsertio_ls", "{}", 1).as_bytes());
    assert!(
        is_error(&broken[0]) && broken[0].contains("corrupt claim"),
        "{}",
        broken[0]
    );
    let attach = talk(
        &dir,
        call(
            "adsertio_attach",
            r#"{"id":"listed","evidence":["probatio:out"]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&attach[0]) && attach[0].contains("corrupt claim"),
        "{}",
        attach[0]
    );
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("verdict: verified"));
}
