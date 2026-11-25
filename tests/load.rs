//! Fourth stress-test fixes: a page does not re-judge the store, a cursor does
//! not skip, and a dead writer's lock or temp file is recovered.
mod common;
use common::*;
use std::process::Command;
use std::time::{Duration, Instant};

fn claim(dir: &std::path::Path, id: &str, created: &str, evidence: &str) {
    let evidence = if evidence.is_empty() {
        String::new()
    } else {
        format!("evidence: [{evidence}]\n")
    };
    let body = format!("---\nsummary: {id}\nauthor: bench\ncreated: {created}\n{evidence}---\n");
    std::fs::write(dir.join(".adsertio/claims").join(format!("{id}.md")), body).unwrap();
}

#[test]
fn a_cursor_does_not_skip_when_an_earlier_verdict_changes() {
    let dir = project("cursor");
    init(&dir);
    std::fs::create_dir_all(dir.join(".trammel")).unwrap();
    std::fs::write(
        dir.join(".trammel/store"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":false}}],"constraints":[],"failures":[]}"#,
    )
    .unwrap();
    for (n, id) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        claim(&dir, id, &format!("2020-01-01T00:00:0{n}Z"), "");
    }
    let first = Command::new(CLI)
        .args([
            "view",
            "--verdict",
            "unverified",
            "--limit",
            "2",
            "--compact",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&first.stdout).to_string();
    assert!(first.status.success(), "{text}");
    assert!(
        text.contains("\"id\":\"a\"") && text.contains("\"id\":\"b\""),
        "{text}"
    );
    assert!(!text.contains("\"id\":\"c\""), "{text}");
    assert!(text.contains("\"counts_complete\":false"), "{text}");
    let cursor = text
        .split("\"next_after\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();
    assert!(cursor.ends_with("/b"), "{cursor}");

    // a was unverified. Point it at a failing step so it becomes refuted.
    claim(
        &dir,
        "a",
        "2020-01-01T00:00:00Z",
        "trammel:.trammel/store#plan-1/step-1",
    );
    let second = Command::new(CLI)
        .args([
            "view",
            "--verdict",
            "unverified",
            "--limit",
            "2",
            "--after",
            &cursor,
            "--compact",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    let page = String::from_utf8_lossy(&second.stdout).to_string();
    assert!(second.status.success(), "{page}");
    assert!(
        page.contains("\"id\":\"c\"") && page.contains("\"id\":\"d\""),
        "{page}"
    );
    assert!(!page.contains("\"id\":\"e\""), "{page}");

    // offset would skip c: a is no longer unverified, so offset 2 starts at d.
    let shifted = Command::new(CLI)
        .args([
            "view",
            "--verdict",
            "unverified",
            "--limit",
            "2",
            "--offset",
            "2",
            "--compact",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    let shifted = String::from_utf8_lossy(&shifted.stdout).to_string();
    assert!(
        shifted.contains("\"id\":\"d\"") && shifted.contains("\"id\":\"e\""),
        "{shifted}"
    );
    assert!(!shifted.contains("\"id\":\"c\""), "{shifted}");
}

#[test]
fn a_page_costs_about_as_much_as_one_claim_on_a_large_store() {
    let dir = project("scale");
    init(&dir);
    let steps = 12_000;
    let claims = 500;
    std::fs::create_dir_all(dir.join(".trammel")).unwrap();
    std::fs::create_dir_all(dir.join(".chisel")).unwrap();
    let mut trammel = String::from("{\"schema_version\":1,\"plans\":[{\"id\":1}],\"steps\":[");
    for i in 1..=steps {
        if i > 1 {
            trammel.push(',');
        }
        trammel.push_str(&format!(
            "{{\"id\":{i},\"plan_id\":1,\"step_index\":{i},\"status\":\"passed\",\"verification\":{{\"passed\":true,\"at\":4102444800,\"command\":\"cargo test\",\"source\":\"working_tree\"}}}}"
        ));
    }
    trammel.push_str("],\"constraints\":[],\"failures\":[]}");
    std::fs::write(dir.join(".trammel/store"), trammel).unwrap();
    let mut chisel = String::from("{\"magic\":\"chisel-store\",\"version\":1,\"test_results\":[");
    for i in 0..steps {
        if i > 0 {
            chisel.push(',');
        }
        chisel.push_str(&format!(
            "{{\"test_id\":\"t{i}\",\"passed\":true,\"recorded_at\":4102444800}}"
        ));
    }
    chisel.push_str("],\"test_edges\":[]}");
    std::fs::write(dir.join(".chisel/store"), chisel).unwrap();
    for i in 0..claims {
        let step = (i % steps) + 1;
        let test = i % steps;
        claim(
            &dir,
            &format!("c{i}"),
            "2020-01-01T00:00:00Z",
            &format!(
                "trammel:.trammel/store#plan-1/step-{step}, chisel:.chisel/store#test:t{test}"
            ),
        );
    }

    let id_time = time_view(&dir, &["view", "--id", "c0", "--compact"]);
    let page_time = time_view(&dir, &["view", "--limit", "1", "--compact"]);
    let check_time = time_view(&dir, &["check"]);
    let allowance = id_time.saturating_mul(5) + Duration::from_millis(800);
    assert!(
        page_time < allowance,
        "page {page_time:?} id {id_time:?} allowance {allowance:?}"
    );
    assert!(
        check_time < id_time.saturating_mul(8) + Duration::from_millis(1500),
        "check {check_time:?} id {id_time:?}"
    );

    let page = Command::new(CLI)
        .args(["view", "--id", "c0", "--compact"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&page.stdout);
    assert!(
        page.status.success() && text.contains("\"verified\""),
        "{text}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn view_rss_stays_well_under_nine_times_the_evidence() {
    let dir = project("rss");
    init(&dir);
    std::fs::create_dir_all(dir.join(".trammel")).unwrap();
    let steps = 40_000;
    let mut body = String::from("{\"schema_version\":1,\"plans\":[{\"id\":1}],\"steps\":[");
    for i in 1..=steps {
        if i > 1 {
            body.push(',');
        }
        body.push_str(&format!(
            "{{\"id\":{i},\"plan_id\":1,\"status\":\"passed\",\"verification\":{{\"passed\":true,\"at\":4102444800,\"command\":\"cargo test --all-targets\",\"source\":\"working_tree\"}}}}"
        ));
    }
    body.push_str("],\"constraints\":[],\"failures\":[]}");
    let path = dir.join(".trammel/store");
    std::fs::write(&path, body).unwrap();
    let bytes = std::fs::metadata(&path).unwrap().len();
    claim(
        &dir,
        "only",
        "2020-01-01T00:00:00Z",
        "trammel:.trammel/store#plan-1/step-1",
    );
    let mut child = Command::new(CLI)
        .args(["view", "--compact"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut peak = 0u64;
    loop {
        let status = child.try_wait().unwrap();
        if let Ok(text) = std::fs::read_to_string(format!("/proc/{}/status", child.id())) {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("VmHWM:") {
                    let kb: u64 = rest.split_whitespace().next().unwrap().parse().unwrap();
                    peak = peak.max(kb);
                }
            }
        }
        if status.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The 1.1.1 tree held every JSON string. 40k small rows were several
    // times the file. The index keeps ids and the pass bit. Headroom covers
    // the binary and the debug allocator.
    let cap_kb = bytes / 1024 * 4 + 24_000;
    assert!(
        peak > 0 && peak < cap_kb,
        "rss {peak} KiB, file {bytes} bytes, cap {cap_kb} KiB"
    );
}

fn time_view(dir: &std::path::Path, args: &[&str]) -> Duration {
    let started = Instant::now();
    let out = Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    assert!(
        out.status.success(),
        "{args:?} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    elapsed
}
