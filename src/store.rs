//! Where claims live: the nearest `.adsertio/` walking up from the working
//! directory, or `$ADSERTIO_DIR`. The evidence it reads belongs to the other
//! tools and sits elsewhere in the project, so the project folder is the
//! parent of `.adsertio/`.
//!
//! ```text
//! .adsertio/
//!   claims/<id>.md        one claim per file
//! ```

use crate::fsx;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Store {
    /// The `.adsertio` folder.
    pub dir: PathBuf,
    /// The project folder: where evidence paths are resolved, and AGENTS.md.
    pub project: PathBuf,
}

impl Store {
    pub fn open() -> Result<Store, String> {
        if let Some(v) = std::env::var_os("ADSERTIO_DIR") {
            return Store::at(PathBuf::from(v));
        }
        let cwd =
            std::env::current_dir().map_err(|e| format!("cannot read the working folder: {e}"))?;
        let mut cur: &Path = &cwd;
        loop {
            let cand = cur.join(".adsertio");
            if let Ok(m) = fs::symlink_metadata(&cand) {
                if m.is_dir() {
                    return Store::at(cand);
                }
            }
            match cur.parent() {
                Some(p) => cur = p,
                None => break,
            }
        }
        Err("no .adsertio folder here or above. Run `adsertio init` in the project folder, or set ADSERTIO_DIR".to_string())
    }

    fn at(dir: PathBuf) -> Result<Store, String> {
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "{} is not a folder (or is a symlink)",
                    dir.display()
                ))
            }
            Err(_) => {
                return Err(format!(
                    "{} does not exist. Run `adsertio init`",
                    dir.display()
                ))
            }
        }
        // `.adsertio` has the parent "", which names no folder.
        let project = dir
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Store { dir, project })
    }

    /// Create `.adsertio/` in the working directory. Safe to repeat.
    pub fn init() -> Result<Store, String> {
        let dir = match std::env::var_os("ADSERTIO_DIR") {
            Some(v) => PathBuf::from(v),
            None => std::env::current_dir()
                .map_err(|e| format!("cannot read the working folder: {e}"))?
                .join(".adsertio"),
        };
        let claims = dir.join("claims");
        if let Ok(meta) = fs::symlink_metadata(&claims) {
            if meta.file_type().is_symlink() {
                return Err(".adsertio/claims is a symlink; refusing to follow it. Run `adsertio init` so claims is a real folder inside the project.".into());
            }
        }
        fsx::make_private_dir(&claims)?;
        Store::at(dir)
    }

    pub fn claims_dir(&self) -> PathBuf {
        self.dir.join("claims")
    }
}

/// The claims folder must be a real directory. A symlink is refused, because
/// following it would read and write outside the project. `create` is only
/// for `init`.
pub fn claims_ready(store: &Store, create: bool) -> Result<(), String> {
    let dir = store.claims_dir();
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_symlink() => Err(
            ".adsertio/claims is a symlink; refusing to follow it. Run `adsertio init` so claims is a real folder inside the project.".into(),
        ),
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(".adsertio/claims is not a folder. Run `adsertio init`.".into()),
        Err(_) if create => fsx::make_private_dir(&dir),
        Err(_) => Err(format!(
            "no claims store found at {}; run init",
            store.dir.display()
        )),
    }
}

pub fn project_dir() -> Option<PathBuf> {
    Store::open().ok().map(|s| s.project)
}
