//! Parsed evidence kept for one `view` call.
//!
//! Many claims often point at the same snapshot. Reading it once per view is
//! enough. The cache is off unless [`with_cache`] is on the stack, so a later
//! read in the same process still sees a file that changed.

use crate::json::Value;
use crate::read::ReadError;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::Arc;

type DocHit = Result<Arc<Value>, ReadError>;
type DocsHit = Result<Arc<Vec<Value>>, ReadError>;
type RunHit = Result<Option<Arc<Vec<String>>>, ReadError>;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static TRAMMEL: RefCell<Vec<(String, DocHit)>> = const { RefCell::new(Vec::new()) };
    static CHISEL: RefCell<Vec<(String, DocsHit)>> = const { RefCell::new(Vec::new()) };
    static PROBATIO: RefCell<Vec<(String, RunHit)>> = const { RefCell::new(Vec::new()) };
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        ON.with(|flag| flag.set(false));
        TRAMMEL.with(|slot| slot.borrow_mut().clear());
        CHISEL.with(|slot| slot.borrow_mut().clear());
        PROBATIO.with(|slot| slot.borrow_mut().clear());
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
    load: impl FnOnce() -> Result<Value, ReadError>,
) -> Result<Arc<Value>, ReadError> {
    if !ON.with(|flag| flag.get()) {
        return load().map(Arc::new);
    }
    let saved = key(project, rel);
    let hit = TRAMMEL.with(|slot| {
        slot.borrow()
            .iter()
            .find(|(name, _)| name == &saved)
            .map(|(_, value)| value.clone())
    });
    if let Some(value) = hit {
        return value;
    }
    let loaded = load().map(Arc::new);
    TRAMMEL.with(|slot| slot.borrow_mut().push((saved, loaded.clone())));
    loaded
}

pub fn chisel(
    project: &Path,
    rel: &str,
    load: impl FnOnce() -> Result<Vec<Value>, ReadError>,
) -> Result<Arc<Vec<Value>>, ReadError> {
    if !ON.with(|flag| flag.get()) {
        return load().map(Arc::new);
    }
    let saved = key(project, rel);
    let hit = CHISEL.with(|slot| {
        slot.borrow()
            .iter()
            .find(|(name, _)| name == &saved)
            .map(|(_, value)| value.clone())
    });
    if let Some(value) = hit {
        return value;
    }
    let loaded = load().map(Arc::new);
    CHISEL.with(|slot| slot.borrow_mut().push((saved, loaded.clone())));
    loaded
}

/// `Ok(None)` means the results directory is absent.
pub fn probatio(
    project: &Path,
    rel: &str,
    load: impl FnOnce() -> Result<Option<Vec<String>>, ReadError>,
) -> Result<Option<Arc<Vec<String>>>, ReadError> {
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
