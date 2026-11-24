//! Undo for file operations (Ctrl+Z).

use std::path::{Path, PathBuf};

const LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A copy was made at `dest`; undo deletes it.
    Copied { dest: PathBuf },
    /// `from` was moved to `to`; undo moves it back. `index` is where it sat
    /// in the folder list.
    Moved {
        from: PathBuf,
        to: PathBuf,
        index: usize,
    },
    /// `path` went to the Recycle Bin; undo restores it.
    Trashed { path: PathBuf, index: usize },
}

#[derive(Default)]
pub struct History {
    stack: Vec<Action>,
}

/// What undo did, so the caller can put the file back into the list.
#[derive(Debug, PartialEq)]
pub enum Undone {
    RemovedCopy,
    Returned { path: PathBuf, index: usize },
}

impl History {
    pub fn push(&mut self, a: Action) {
        self.stack.push(a);
        if self.stack.len() > LIMIT {
            self.stack.remove(0);
        }
    }

    /// Reverts the most recent action. On failure the action stays on the stack.
    pub fn undo(&mut self) -> Option<Result<Undone, String>> {
        let action = self.stack.pop()?;
        let result = revert(&action);
        if result.is_err() {
            self.stack.push(action);
        }
        Some(result)
    }
}

fn revert(a: &Action) -> Result<Undone, String> {
    match a {
        Action::Copied { dest } => {
            std::fs::remove_file(dest).map_err(|e| e.to_string())?;
            Ok(Undone::RemovedCopy)
        }
        Action::Moved { from, to, index } => {
            if from.exists() {
                return Err("a file with the original name already exists".into());
            }
            super::photo_folders::move_file(to, from).map_err(|e| e.to_string())?;
            Ok(Undone::Returned {
                path: from.clone(),
                index: *index,
            })
        }
        Action::Trashed { path, index } => {
            restore_from_trash(path)?;
            Ok(Undone::Returned {
                path: path.clone(),
                index: *index,
            })
        }
    }
}

#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
fn restore_from_trash(path: &Path) -> Result<(), String> {
    use trash::os_limited::{list, restore_all};
    let parent = path.parent().ok_or("no parent folder")?;
    let name = path.file_name().ok_or("no file name")?;
    let newest = list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|i| i.name == name && i.original_parent == parent)
        .max_by_key(|i| i.time_deleted)
        .ok_or("not found in the Recycle Bin")?;
    restore_all([newest]).map_err(|e| e.to_string())
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn restore_from_trash(_path: &Path) -> Result<(), String> {
    Err("restoring from the trash is not supported on this system".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "muud-hist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn undo_copy_and_move() {
        let d = temp_dir();
        let a = d.join("a.jpg");
        std::fs::write(&a, b"x").unwrap();
        let copy = super::super::photo_folders::copy_to(&a, &d.join("box")).unwrap();
        let mut h = History::default();
        h.push(Action::Copied { dest: copy.clone() });
        let moved = super::super::photo_folders::move_to(&a, &d.join("box")).unwrap();
        h.push(Action::Moved {
            from: a.clone(),
            to: moved.clone(),
            index: 4,
        });

        assert_eq!(
            h.undo(),
            Some(Ok(Undone::Returned {
                path: a.clone(),
                index: 4
            }))
        );
        assert!(a.exists() && !moved.exists());
        assert_eq!(h.undo(), Some(Ok(Undone::RemovedCopy)));
        assert!(!copy.exists());
        assert!(h.undo().is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn failed_undo_stays_on_stack() {
        let mut h = History::default();
        h.push(Action::Copied {
            dest: PathBuf::from("/definitely/not/here.jpg"),
        });
        assert!(matches!(h.undo(), Some(Err(_))));
        assert_eq!(h.stack.len(), 1);
    }

    #[test]
    fn keeps_last_fifty() {
        let mut h = History::default();
        for i in 0..60 {
            h.push(Action::Copied {
                dest: PathBuf::from(format!("{i}")),
            });
        }
        assert_eq!(h.stack.len(), LIMIT);
        assert_eq!(
            h.stack[0],
            Action::Copied {
                dest: PathBuf::from("10")
            }
        );
    }
}
