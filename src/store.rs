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
        let project = dir
            .parent()
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
        fsx::make_private_dir(&dir.join("claims"))?;
        Store::at(dir)
    }

    pub fn claims_dir(&self) -> PathBuf {
        self.dir.join("claims")
    }
}

pub fn project_dir() -> Option<PathBuf> {
    Store::open().ok().map(|s| s.project)
}
