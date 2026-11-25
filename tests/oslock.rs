//! Attach locks are operating-system locks: a second attach waits for the
//! first and keeps both pointers, a killed holder frees the lock at once, an
//! attach to another claim is not held up, and lock files from 1.3 block
//! nothing.
//!
//! The holder is this test binary run again with `ADSERTIO_TEST_HOLD` set to
//! a claim id: it takes that claim's attach lock, prints `held`, waits for a
//! `release` file in the project folder, then adds `ADSERTIO_TEST_POINTER`
//! (when set) to the claim while still holding the lock, as an attach does.
//! (Not stdin: a pipe end can leak into a child that another test thread
//! starts at the same moment, and then EOF never comes.)

mod common;
use adsertio::claim::Claim;
use adsertio::fsx;
use adsertio::store::Store;
use common::*;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Not a test of its own: the body runs only in a child started by `hold`.
#[test]
fn holder() {
    let Ok(id) = std::env::var("ADSERTIO_TEST_HOLD") else {
        return;
    };
    let store = Store::open().unwrap();
    let lock = store.attach_lock(&id).unwrap();
    fsx::with_claim_lock(&lock, || {
        println!("held");
        let release = store.project.join("release");
        let start = Instant::now();
        while !release.exists() && start.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Ok(pointer) = std::env::var("ADSERTIO_TEST_POINTER") {
            let path = store.claims_dir().join(format!("{id}.md"));
            let mut claim = Claim::parse(&id, &fs::read_to_string(&path).unwrap())?;
            claim.evidence.push(pointer);
            fsx::write_atomic(&path, claim.render().as_bytes(), true)?;
        }
        Ok(())
    })
    .unwrap();
}

/// Start a process that holds claim `id`'s attach lock in the store under
/// `dir`, and return once it holds it.
fn hold(dir: &Path, id: &str, pointer: Option<&str>) -> Child {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "holder", "--nocapture", "--test-threads=1"])
        .env("ADSERTIO_TEST_HOLD", id)
        .env("ADSERTIO_DIR", dir.join(".adsertio"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(pointer) = pointer {
        cmd.env("ADSERTIO_TEST_POINTER", pointer);
    }
    let mut child = cmd.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    loop {
        match lines.next() {
            // The harness may print "test holder ... " first on the same line.
            Some(Ok(line)) if line.trim_end().ends_with("held") => break,
            Some(Ok(_)) => continue,
            _ => panic!("the holder exited before it held {id}"),
        }
    }
    // Keep reading so the holder never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    child
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The doctor report. It exits 1 here whatever the store holds: the test
/// project is not a git repository.
fn doctor(dir: &Path) -> String {
    String::from_utf8_lossy(&cli(dir, &["doctor"]).stdout).into_owned()
}

fn claim(dir: &Path, id: &str) {
    ok(&cli(dir, &["claim", "--summary", "Locked?", "--id", id]));
}

fn attach(dir: &Path, id: &str, pointer: &str) -> Output {
    cli(dir, &["attach", "--id", id, "--evidence", pointer])
}

fn body(dir: &Path, id: &str) -> String {
    fs::read_to_string(dir.join(format!(".adsertio/claims/{id}.md"))).unwrap()
}

#[test]
fn a_second_attach_waits_for_the_first_and_both_pointers_are_kept() {
    let dir = project("os-contend");
    init(&dir);
    claim(&dir, "held");
    claim(&dir, "other");
    let mut holder = hold(&dir, "held", Some("trammel:.trammel/store#plan-1"));
    let path = dir.join(".adsertio/locks/held.lock");
    // Busy for anyone else, in this process too.
    assert!(adsertio::oslock::FileLock::try_acquire(&path)
        .unwrap()
        .is_none());

    // Another claim has its own lock: no wait.
    let started = Instant::now();
    ok(&attach(&dir, "other", "trammel:.trammel/store#plan-9"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "an attach to another claim waited {:?}",
        started.elapsed()
    );

    let started = Instant::now();
    let mut second = Command::new(CLI)
        .args([
            "attach",
            "--id",
            "held",
            "--evidence",
            "trammel:.trammel/store#plan-2",
        ])
        .current_dir(&dir)
        .env_remove("ADSERTIO_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        second.try_wait().unwrap().is_none(),
        "the second attach did not wait for the held lock"
    );
    fs::write(dir.join("release"), b"").unwrap();
    assert!(holder.wait().unwrap().success());
    ok(&second.wait_with_output().unwrap());
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert!(started.elapsed() < Duration::from_secs(5));
    let text = body(&dir, "held");
    assert!(
        text.contains("#plan-1") && text.contains("#plan-2"),
        "{text}"
    );
    assert!(path.is_file(), "a lock file is never deleted");
    // The lock files are part of the store: doctor and check pass with them.
    ok(&cli(&dir, &["check"]));
    assert!(path.is_file());
    let doc = doctor(&dir);
    assert!(
        doc.contains("\"name\":\"leftover_files\",\"ok\":true"),
        "{doc}"
    );
}

#[test]
fn a_killed_holder_frees_the_lock_at_once() {
    let dir = project("os-kill");
    init(&dir);
    claim(&dir, "held");
    let mut holder = hold(&dir, "held", Some("trammel:.trammel/store#plan-lost"));
    // SIGKILL on Unix, TerminateProcess on Windows: no cleanup runs.
    holder.kill().unwrap();
    holder.wait().unwrap();
    let started = Instant::now();
    ok(&attach(&dir, "held", "trammel:.trammel/store#plan-after"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "attach waited {:?} after its holder was killed",
        started.elapsed()
    );
    let text = body(&dir, "held");
    assert!(text.contains("#plan-after"), "{text}");
    assert!(!text.contains("#plan-lost"), "{text}");
    assert!(dir.join(".adsertio/locks/held.lock").is_file());
}

#[test]
fn an_attach_to_a_missing_claim_makes_no_lock_file() {
    let dir = project("os-missing");
    init(&dir);
    let out = attach(&dir, "nope", "trammel:.trammel/store#plan-1");
    assert!(!out.status.success());
    assert!(!dir.join(".adsertio/locks/nope.lock").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_locks_folder_is_refused() {
    let dir = project("os-symlink");
    init(&dir);
    claim(&dir, "held");
    let outside = project("os-symlink-outside");
    std::os::unix::fs::symlink(&outside, dir.join(".adsertio/locks")).unwrap();
    let out = attach(&dir, "held", "trammel:.trammel/store#plan-1");
    assert!(!out.status.success());
    let msg =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    assert!(msg.contains("symlink"), "{msg}");
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
}

#[test]
fn lock_files_from_1_3_block_nothing_and_check_removes_them() {
    let dir = project("os-old");
    init(&dir);
    claim(&dir, "held");
    // 1.3 lock files naming a live process (this one): 1.3 would have waited
    // and failed after 5 seconds.
    let line = format!("{} 0 0-0\n", std::process::id());
    let old = [
        ".adsertio/claims/.held.md.lock",
        ".adsertio/claims/.held.md.lock.break",
    ];
    for rel in old {
        fs::write(dir.join(rel), &line).unwrap();
    }
    let started = Instant::now();
    ok(&attach(&dir, "held", "trammel:.trammel/store#plan-1"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(body(&dir, "held").contains("#plan-1"));
    // view and ls skip them quietly; doctor reports them and changes nothing.
    let view = cli(&dir, &["view"]);
    ok(&view);
    assert!(!String::from_utf8_lossy(&view.stderr).contains(".held.md.lock"));
    let doc = doctor(&dir);
    assert!(
        doc.contains("\"name\":\"leftover_files\",\"ok\":false"),
        "{doc}"
    );
    assert!(doc.contains(".held.md.lock.break"), "{doc}");
    assert!(old.iter().all(|rel| dir.join(rel).exists()));
    // check removes them and passes.
    let checked = ok(&cli(&dir, &["check"]));
    for rel in old {
        assert!(!dir.join(rel).exists(), "{rel}: {checked}");
    }
    assert!(dir.join(".adsertio/locks/held.lock").is_file());
    let doc = doctor(&dir);
    assert!(
        doc.contains("\"name\":\"leftover_files\",\"ok\":true"),
        "{doc}"
    );
}

#[test]
fn temp_files_are_reaped_by_age_only() {
    let dir = project("os-temp");
    init(&dir);
    claim(&dir, "held");
    let claims = dir.join(".adsertio/claims");
    let set_age = |p: &Path, secs: u64| {
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(secs))
            .unwrap();
    };
    // Over an hour old: gone, whatever pid it names (this live one).
    let old = claims.join(format!(".held.md.tmp-{}-1", std::process::id()));
    fs::write(&old, b"leftover").unwrap();
    set_age(&old, 2 * 60 * 60);
    // Fresh, naming a pid that is not running: kept, a writer may own it.
    let fresh = claims.join(".held.md.tmp-999999-1");
    fs::write(&fresh, b"in progress").unwrap();
    set_age(&fresh, 120);
    let out = ok(&cli(&dir, &["check"]));
    assert!(!old.exists(), "{out}");
    assert!(fresh.exists(), "{out}");
    assert!(out.contains(".held.md.tmp-999999-1"), "{out}");
}
