//! Noticing changes made outside GitCraft: by an editor, a build or `git` in a terminal.

use std::path::{Path, PathBuf};

use gix::worktree::Stack;
use gix::worktree::stack::state::ignore::Source;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::repo::Repo;
use crate::{Error, Result};

/// Watches a repository until it is dropped.
#[derive(Debug)]
pub struct Watch {
    _watcher: RecommendedWatcher,
}

impl Repo {
    /// Calls `changed` whenever something changes that GitCraft shows: a file in the working tree
    /// that git does not ignore, a branch, a tag, `HEAD` or the index. Reading files does not
    /// count.
    pub fn watch(&self, changed: impl Fn() + Send + 'static) -> Result<Watch> {
        let canonical = |dir: &Path| dir.canonicalize().map_err(Error::watch);
        let git_dirs = vec![canonical(self.gix.git_dir())?, canonical(self.gix.common_dir())?];
        let workdir = self.gix.workdir().map(canonical).transpose()?;
        let mut dirs: Vec<_> = workdir.iter().chain(&git_dirs).cloned().collect();
        // A folder inside another one is watched with it.
        dirs.sort();
        dirs.dedup_by(|dir, outer| dir.starts_with(outer));
        let mut filter = Filter { repo: self.gix.clone(), git_dirs, workdir, ignored: None };
        let mut watcher = notify::recommended_watcher(move |event| {
            if filter.matters(event) {
                changed();
            }
        })
        .map_err(Error::watch)?;
        for dir in dirs {
            watcher.watch(&dir, RecursiveMode::Recursive).map_err(Error::watch)?;
        }
        Ok(Watch { _watcher: watcher })
    }
}

/// Tells the changes that matter from the ones that do not.
struct Filter {
    repo: gix::Repository,
    git_dirs: Vec<PathBuf>,
    workdir: Option<PathBuf>,
    /// The ignore rules, read when first needed and again after they change.
    ignored: Option<Stack>,
}

impl Filter {
    fn matters(&mut self, event: notify::Result<Event>) -> bool {
        // When the events themselves went wrong, anything may have changed.
        let Ok(event) = event else { return true };
        let debug = format!("{event:?}");
        let result = event.need_rescan()
            || !matches!(event.kind, EventKind::Access(_)) && event.paths.iter().any(|p| self.path(p));
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 100_000;
        std::io::Write::write_all(&mut std::io::stderr(), format!("WATCH {ms} {result} {debug}\n").as_bytes()).unwrap();
        result
    }

    fn path(&mut self, path: &Path) -> bool {
        if let Some(inside) = self.git_dirs.iter().find_map(|dir| path.strip_prefix(dir).ok()) {
            if inside == Path::new("info/exclude") {
                self.ignored = None;
            }
            // Objects and logs change along with a ref; a lock file just before the file it locks.
            return !(inside.starts_with("objects")
                || inside.starts_with("logs")
                || path.extension().is_some_and(|e| e == "lock"));
        }
        let Some(relative) = self.workdir.as_deref().and_then(|dir| path.strip_prefix(dir).ok()) else {
            return true;
        };
        if path.file_name().is_some_and(|name| name == ".gitignore") {
            self.ignored = None;
        }
        // A file whose rules cannot be read counts as not ignored: reloading is always safe.
        !self.is_ignored(relative).unwrap_or(false)
    }

    fn is_ignored(&mut self, relative: &Path) -> Result<bool> {
        let stack = if let Some(stack) = &mut self.ignored {
            stack
        } else {
            let index = self.repo.index_or_empty().map_err(Error::repository)?;
            let rules = self.repo.excludes(&index, None, Source::default()).map_err(Error::repository)?;
            self.ignored.insert(rules.detach())
        };
        Ok(stack.at_path(relative, None, &self.repo.objects)?.is_excluded())
    }
}
