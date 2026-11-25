//! Parsed evidence kept for one `view` or `check` call.
//!
//! Many claims often point at the same snapshot. Reading and indexing it once
//! per call is enough. The cache is off unless [`with_cache`] is on the stack,
//! so a later read in the same process still sees a file that changed.

use crate::chisel_idx;
use crate::read::ReadError;
use crate::trammel_idx;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::Arc;

type TrammelHit = Result<Arc<trammel_idx::Index>, ReadError>;
type ChiselHit = Result<Arc<chisel_idx::Index>, ReadError>;
/// Each result as (mutant id, outcome).
pub type RunRows = Vec<(String, String)>;
type RunHit = Result<Option<Arc<RunRows>>, ReadError>;
type HeadHit = Result<(String, String), String>;
type TreeHit = Result<bool, String>;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static TRAMMEL: RefCell<Vec<(String, TrammelHit)>> = const { RefCell::new(Vec::new()) };
    static CHISEL: RefCell<Vec<(String, ChiselHit)>> = const { RefCell::new(Vec::new()) };
    static PROBATIO: RefCell<Vec<(String, RunHit)>> = const { RefCell::new(Vec::new()) };
    static HEAD: RefCell<Vec<(String, HeadHit)>> = const { RefCell::new(Vec::new()) };
    static TREE: RefCell<Vec<(String, TreeHit)>> = const { RefCell::new(Vec::new()) };
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        ON.with(|flag| flag.set(false));
        TRAMMEL.with(|slot| slot.borrow_mut().clear());
        CHISEL.with(|slot| slot.borrow_mut().clear());
        PROBATIO.with(|slot| slot.borrow_mut().clear());
        HEAD.with(|slot| slot.borrow_mut().clear());
        TREE.with(|slot| slot.borrow_mut().clear());
    }
}

/// Turn the cache on for `body`, then drop everything it stored.
pub fn with_cache<T>(body: impl FnOnce() -> T) -> T {
    ON.with(|flag| flag.set(true));
    let _reset = Reset;
    body()
}

fn key(project: &Path, rel: &str) -> String {
    format!("{}|{rel}", project.display())
}

pub fn trammel(
    project: &Path,
    rel: &str,
    load: impl FnOnce() -> Result<trammel_idx::Index, ReadError>,
) -> Result<Arc<trammel_idx::Index>, ReadError> {
    remember(&TRAMMEL, project, rel, load)
}

pub fn chisel(
    project: &Path,
    rel: &str,
    load: impl FnOnce() -> Result<chisel_idx::Index, ReadError>,
) -> Result<Arc<chisel_idx::Index>, ReadError> {
    remember(&CHISEL, project, rel, load)
}

type Slot<T> = std::thread::LocalKey<RefCell<Vec<(String, Result<Arc<T>, ReadError>)>>>;

fn remember<T>(
    slot: &'static Slot<T>,
    project: &Path,
    rel: &str,
    load: impl FnOnce() -> Result<T, ReadError>,
) -> Result<Arc<T>, ReadError> {
    if !ON.with(|flag| flag.get()) {
        return load().map(Arc::new);
    }
    let saved = key(project, rel);
    let hit = slot.with(|cell| {
        cell.borrow()
            .iter()
            .find(|(name, _)| name == &saved)
            .map(|(_, value)| value.clone())
    });
    if let Some(value) = hit {
        return value;
    }
    let loaded = load().map(Arc::new);
    slot.with(|cell| cell.borrow_mut().push((saved, loaded.clone())));
    loaded
}

/// `Ok(None)` means the results directory is absent.
pub fn probatio(
    project: &Path,
    rel: &str,
    load: impl FnOnce() -> Result<Option<RunRows>, ReadError>,
) -> RunHit {
    if !ON.with(|flag| flag.get()) {
        return load().map(|rows| rows.map(Arc::new));
    }
    let saved = key(project, rel);
    let hit = PROBATIO.with(|slot| {
        slot.borrow()
            .iter()
            .find(|(name, _)| name == &saved)
            .map(|(_, value)| value.clone())
    });
    if let Some(value) = hit {
        return value;
    }
    let loaded = load().map(|rows| rows.map(Arc::new));
    PROBATIO.with(|slot| slot.borrow_mut().push((saved, loaded.clone())));
    loaded
}

/// `git rev-parse HEAD HEAD^{tree}` once per project per call: HEAD and its tree.
pub fn head(
    project: &Path,
    load: impl FnOnce() -> Result<(String, String), String>,
) -> Result<(String, String), String> {
    if !ON.with(|flag| flag.get()) {
        return load();
    }
    let saved = project.display().to_string();
    let hit = HEAD.with(|slot| {
        slot.borrow()
            .iter()
            .find(|(name, _)| name == &saved)
            .map(|(_, value)| value.clone())
    });
    if let Some(value) = hit {
        return value;
    }
    let loaded = load();
    HEAD.with(|slot| slot.borrow_mut().push((saved, loaded.clone())));
    loaded
}

/// Whether a tested tree differs from HEAD's only in tool state, once per
/// project and tree per call.
pub fn tree_diff(
    project: &Path,
    tree: &str,
    load: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    if !ON.with(|flag| flag.get()) {
        return load();
    }
    let saved = key(project, tree);
    let hit = TREE.with(|slot| {
        slot.borrow()
            .iter()
            .find(|(name, _)| name == &saved)
            .map(|(_, value)| value.clone())
    });
    if let Some(value) = hit {
        return value;
    }
    let loaded = load();
    TREE.with(|slot| slot.borrow_mut().push((saved, loaded.clone())));
    loaded
}
