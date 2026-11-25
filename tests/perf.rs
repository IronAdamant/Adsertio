//! Release-mode timings for the hot paths. Ignored by `cargo test`.
//!
//! ```sh
//! cargo test --release --locked --test perf -- --ignored --nocapture
//! ```
//!
//! Prints median and p95 in microseconds, and the largest files it built.
#![allow(dead_code)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CLI: &str = env!("CARGO_BIN_EXE_adsertio");
const MCP: &str = env!("CARGO_BIN_EXE_adsertio-mcp");

#[ignore]
#[test]
fn perf_baseline() {
    print_machine();
    let scratch = std::env::temp_dir().join(format!("adsertio-perf-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();

    time_cold_version();
    let mcp_dir = project(&scratch, "mcp");
    init(&mcp_dir);
    time_mcp(&mcp_dir);

    for n in [10usize, 1_000, 10_000] {
        let dir = project(&scratch, &format!("claims-{n}"));
        init(&dir);
        seed_claims(&dir, n, None);
        println!(
            "SIZE claims {n} dir_bytes {} max_file {}",
            dir_bytes(&dir.join(".adsertio")),
            max_file(&dir.join(".adsertio"))
        );
        report(&format!("ls {n}"), &time_cli(&dir, &["ls"], 11));
        report(
            &format!("view {n}"),
            &time_cli(&dir, &["view"], if n == 10_000 { 5 } else { 9 }),
        );
        report(
            &format!("view_id {n}"),
            &time_cli(&dir, &["view", "--id", "c0"], 9),
        );
        report(&format!("claim {n}"), &time_claim(&dir, n, 7));
        report(&format!("attach {n}"), &time_attach(&dir, 7));
    }

    for n in [10usize, 1_000, 10_000] {
        let dir = project(&scratch, &format!("trammel-{n}"));
        init(&dir);
        let bytes = write_trammel(&dir, n);
        seed_claims(
            &dir,
            1,
            Some(&format!("trammel:.trammel/store#plan-1/step-{n}")),
        );
        println!("SIZE trammel_steps {n} file_bytes {bytes}");
        report(
            &format!("view_trammel {n}"),
            &time_cli(&dir, &["view"], if n == 10_000 { 5 } else { 9 }),
        );
        report(
            &format!("read_trammel {n}"),
            &time_fn(if n == 10_000 { 5 } else { 9 }, || {
                let _ = adsertio::read::read_trammel(&dir, ".trammel/store").unwrap();
            }),
        );
    }

    for n in [10usize, 1_000, 10_000] {
        let dir = project(&scratch, &format!("chisel-{n}"));
        init(&dir);
        let bytes = write_chisel(&dir, n);
        seed_claims(&dir, 1, Some("chisel:.chisel/store#test:t-target"));
        println!("SIZE chisel_results {n} file_bytes {bytes}");
        report(
            &format!("view_chisel {n}"),
            &time_cli(&dir, &["view"], if n == 10_000 { 5 } else { 9 }),
        );
    }

    for n in [10usize, 1_000, 10_000] {
        let dir = project(&scratch, &format!("probatio-{n}"));
        init(&dir);
        let bytes = write_probatio(&dir, n);
        seed_claims(&dir, 1, Some("probatio:out"));
        println!("SIZE probatio_results {n} dir_bytes {bytes}");
        report(
            &format!("view_probatio {n}"),
            &time_cli(&dir, &["view"], if n == 10_000 { 3 } else { 7 }),
        );
    }

    for n in [10usize, 1_000, 10_000] {
        let dir = project(&scratch, &format!("ledger-{n}"));
        init(&dir);
        let bytes = write_ledger(&dir, n);
        seed_claims(&dir, 1, Some("probatio:out#ledger"));
        println!("SIZE ledger_entries {n} file_bytes {bytes}");
        report(
            &format!("view_ledger {n}"),
            &time_cli(&dir, &["view"], if n == 10_000 { 5 } else { 9 }),
        );
    }

    // Many claims sharing one store: the repeated full read.
    let dir = project(&scratch, "shared");
    init(&dir);
    let bytes = write_trammel(&dir, 1_000);
    seed_claims(&dir, 100, Some("trammel:.trammel/store#plan-1/step-1000"));
    println!("SIZE shared_100_on_trammel_1000 file_bytes {bytes}");
    report("view_shared_100", &time_cli(&dir, &["view"], 5));
}

fn print_machine() {
    let cpu = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model = cpu
        .lines()
        .find(|line| line.starts_with("model name"))
        .unwrap_or("model name: unknown");
    let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let total = mem.lines().next().unwrap_or("MemTotal: unknown");
    let uname = Command::new("uname").args(["-srm"]).output().unwrap();
    println!("MACHINE {}", String::from_utf8_lossy(&uname.stdout).trim());
    println!("MACHINE {model}");
    println!("MACHINE {total}");
    println!("MACHINE rustc {}", rustc_version());
}

fn rustc_version() -> String {
    let out = Command::new("rustc").arg("--version").output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn project(scratch: &Path, name: &str) -> PathBuf {
    let dir = scratch.join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn init(dir: &Path) {
    let out = Command::new(CLI)
        .arg("init")
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn seed_claims(dir: &Path, n: usize, evidence: Option<&str>) {
    let claims = dir.join(".adsertio/claims");
    fs::create_dir_all(&claims).unwrap();
    for i in 0..n {
        let evidence = match evidence {
            Some(pointer) => format!("evidence: [{pointer}]\n"),
            None => String::new(),
        };
        let body = format!(
            "---\nsummary: claim {i}\nauthor: bench\ncreated: 2020-01-01T00:00:00Z\n{evidence}---\n"
        );
        fs::write(claims.join(format!("c{i}.md")), body).unwrap();
    }
}

fn write_trammel(dir: &Path, n: usize) -> u64 {
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    let mut steps = String::new();
    for i in 1..n {
        steps.push_str(&format!(
            r#"{{"id":{i},"plan_id":2,"step_index":{i},"status":"pending"}},"#
        ));
    }
    steps.push_str(&format!(
        r#"{{"id":{n},"plan_id":1,"step_index":1,"status":"passed","verification":{{"passed":true,"at":4102444800,"command":"cargo test","source":"working_tree"}}}}"#
    ));
    let doc = format!(
        r#"{{"schema_version":1,"plans":[{{"id":1}}],"steps":[{steps}],"constraints":[],"failures":[]}}"#
    );
    let path = dir.join(".trammel/store");
    fs::write(&path, doc).unwrap();
    fs::metadata(&path).unwrap().len()
}

fn write_chisel(dir: &Path, n: usize) -> u64 {
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    let mut rows = String::new();
    for i in 0..n.saturating_sub(1) {
        rows.push_str(&format!(
            r#"{{"test_id":"t{i}","passed":true,"recorded_at":4102444800}},"#
        ));
    }
    rows.push_str(r#"{"test_id":"t-target","passed":true,"recorded_at":4102444800}"#);
    let doc = format!(
        r#"{{"magic":"chisel-store","version":1,"test_results":[{rows}],"test_edges":[]}}"#
    );
    let path = dir.join(".chisel/store");
    fs::write(&path, doc).unwrap();
    fs::metadata(&path).unwrap().len()
}

fn write_probatio(dir: &Path, n: usize) -> u64 {
    let results = dir.join("out/results");
    fs::create_dir_all(&results).unwrap();
    let mut total = 0u64;
    for i in 0..n {
        let body = format!(r#"{{"id":"m{i}","outcome":"killed","file":"src/a.rs","line":1}}"#);
        let path = results.join(format!("m{i}.json"));
        fs::write(&path, &body).unwrap();
        total += body.len() as u64;
    }
    total
}

fn write_ledger(dir: &Path, n: usize) -> u64 {
    fs::create_dir_all(dir.join("out")).unwrap();
    let mut entries = String::from("[");
    for i in 0..n {
        if i > 0 {
            entries.push(',');
        }
        entries.push_str(&format!(
            r#"{{"id":"e{i}","status":"clean","commit":"abc"}}"#
        ));
    }
    entries.push(']');
    let doc =
        format!(r#"{{"commit":"abc","package":"pkg","testsDir":"tests","entries":{entries}}}"#);
    let path = dir.join("out/ledger.json");
    fs::write(&path, &doc).unwrap();
    doc.len() as u64
}

fn time_cold_version() {
    report(
        "cold_version",
        &time_fn(31, || {
            let out = Command::new(CLI).arg("--version").output().unwrap();
            assert!(out.status.success());
        }),
    );
}

fn time_mcp(dir: &Path) {
    report("mcp_cold_ls", &time_fn(15, || mcp_cold(dir)));
    report("mcp_warm_ls", &mcp_warm(dir, 31));
}

fn mcp_cold(dir: &Path) {
    let mut child = Command::new(MCP)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"adsertio_ls","arguments":{}}}"#)
        .unwrap();
    stdin.write_all(b"\n").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty());
}

fn mcp_warm(dir: &Path, samples: usize) -> Vec<Duration> {
    let mut child = Command::new(MCP)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let line = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"adsertio_ls\",\"arguments\":{}}}\n";
    stdin.write_all(line).unwrap();
    stdin.flush().unwrap();
    let mut buf = String::new();
    reader.read_line(&mut buf).unwrap();
    assert!(buf.contains("adsertio_ls") || buf.contains("claims") || buf.contains("isError"));
    let mut samples_out = Vec::with_capacity(samples);
    for _ in 0..samples {
        buf.clear();
        let start = Instant::now();
        stdin.write_all(line).unwrap();
        stdin.flush().unwrap();
        reader.read_line(&mut buf).unwrap();
        samples_out.push(start.elapsed());
        assert!(!buf.is_empty());
    }
    drop(stdin);
    let _ = child.wait();
    samples_out
}

fn time_cli(dir: &Path, args: &[&str], samples: usize) -> Vec<Duration> {
    time_fn(samples, || {
        let out = Command::new(CLI)
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{} {}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
    })
}

fn time_claim(dir: &Path, already: usize, samples: usize) -> Vec<Duration> {
    let mut out = Vec::with_capacity(samples);
    for i in 0..samples {
        let id = format!("w{already}{i}");
        let start = Instant::now();
        let cmd = Command::new(CLI)
            .args(["claim", "--summary", "timed write", "--id", &id])
            .current_dir(dir)
            .output()
            .unwrap();
        let elapsed = start.elapsed();
        assert!(
            cmd.status.success(),
            "{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
        fs::remove_file(dir.join(".adsertio/claims").join(format!("{id}.md"))).unwrap();
        out.push(elapsed);
    }
    out
}

fn time_attach(dir: &Path, samples: usize) -> Vec<Duration> {
    let path = dir.join(".adsertio/claims/c0.md");
    let original = fs::read(&path).unwrap();
    let mut out = Vec::with_capacity(samples);
    for _ in 0..samples {
        fs::write(&path, &original).unwrap();
        let start = Instant::now();
        let cmd = Command::new(CLI)
            .args([
                "attach",
                "--id",
                "c0",
                "--evidence",
                "probatio:out/missing#survivors",
            ])
            .current_dir(dir)
            .output()
            .unwrap();
        let elapsed = start.elapsed();
        assert!(
            cmd.status.success(),
            "{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
        out.push(elapsed);
    }
    fs::write(&path, &original).unwrap();
    out
}

fn time_fn(samples: usize, mut body: impl FnMut()) -> Vec<Duration> {
    let mut out = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        body();
        out.push(start.elapsed());
    }
    out
}

fn report(name: &str, samples: &[Duration]) {
    let mut sorted = samples.to_vec();
    sorted.sort();
    let median = sorted[sorted.len() / 2];
    let rank = ((sorted.len() as f64) * 0.95).ceil() as usize;
    let p95 = sorted[rank.saturating_sub(1).min(sorted.len() - 1)];
    println!(
        "BENCH {name} n {} median_us {} p95_us {}",
        samples.len(),
        median.as_micros(),
        p95.as_micros()
    );
}

fn dir_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(p);
            } else {
                total += meta.len();
            }
        }
    }
    total
}

fn max_file(path: &Path) -> u64 {
    let mut max = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(p);
            } else {
                max = max.max(meta.len());
            }
        }
    }
    max
}
