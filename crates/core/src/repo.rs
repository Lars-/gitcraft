//! An open repository: its history, its commits and the `git` to change it with.

use std::collections::HashMap;
use std::path::Path;

use gix::ObjectId;
use gix::refs::Category;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::git::Git;
use crate::graph::{Graph, Parents, date_order};
use crate::{Error, Result};

/// An open repository.
#[derive(Debug, Clone)]
pub struct Repo {
    /// For reading, in-process.
    pub gix: gix::Repository,
    /// For everything else.
    pub git: Git,
}

/// What kind of ref a [`Label`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    /// A branch in this repository.
    Local,
    /// A branch on a remote, as of the last fetch.
    Remote,
    /// A tag.
    Tag,
}

/// A branch or tag on a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The short name, like `main`, `origin/main` or `v1.0`.
    pub name: String,
    /// What kind of ref it is.
    pub kind: RefKind,
}

/// Every commit reachable from a branch, a tag or `HEAD`, children before parents.
#[derive(Debug, Clone, Default)]
pub struct History {
    /// The commit in every row.
    pub ids: Vec<ObjectId>,
    /// The layout of the rows.
    pub graph: Graph,
    /// The branches and tags, by row.
    pub labels: HashMap<usize, Vec<Label>>,
    /// The row `HEAD` points at.
    pub head: Option<usize>,
}

impl History {
    /// The row of a commit.
    pub fn row(&self, id: ObjectId) -> Option<usize> {
        self.ids.iter().position(|&i| i == id)
    }
}

/// The parts of a commit the app shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The whole message.
    pub message: String,
    /// The author's name.
    pub author: String,
    /// The author's email address.
    pub email: String,
    /// When it was authored.
    pub time: gix::date::Time,
}

impl Commit {
    /// The first line of the message.
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }
}

impl Repo {
    /// Opens the repository that contains `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut gix = gix::discover(path).map_err(Error::repository)?;
        gix.object_cache_size_if_unset(64 << 20);
        let dir = gix.workdir().unwrap_or_else(|| gix.git_dir()).to_owned();
        Ok(Self { gix, git: Git::new(dir) })
    }

    /// The name of the repository's folder.
    pub fn name(&self) -> String {
        self.git
            .dir()
            .file_name()
            .map_or_else(|| self.git.dir().display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// Reads the whole history.
    pub fn history(&self) -> Result<History> {
        let mut labels = Vec::new();
        for reference in self.gix.references().map_err(Error::repository)?.all().map_err(Error::repository)? {
            let mut reference = reference.map_err(Error::repository)?;
            let Some((category, name)) = reference.name().category_and_short_name() else {
                continue;
            };
            let kind = match category {
                Category::LocalBranch => RefKind::Local,
                Category::RemoteBranch => RefKind::Remote,
                Category::Tag => RefKind::Tag,
                _ => continue,
            };
            let name = name.to_string();
            // `origin/HEAD` only points at another remote branch; tags on trees are not history.
            if name.ends_with("/HEAD") {
                continue;
            }
            if let Ok(commit) = reference.peel_to_commit() {
                labels.push((commit.id, Label { name, kind }));
            }
        }
        let head = self.gix.head_id().ok().map(gix::Id::detach);
        let tips: Vec<_> = labels.iter().map(|(id, _)| *id).chain(head).collect();

        let mut ids = Vec::new();
        let mut times = Vec::new();
        let mut parent_ids = Vec::new();
        for info in self
            .gix
            .rev_walk(tips)
            .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
            .all()
            .map_err(Error::repository)?
        {
            let info = info.map_err(Error::repository)?;
            ids.push(info.id);
            times.push(info.commit_time());
            parent_ids.push(info.parent_ids);
        }
        let index: HashMap<_, _> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        let mut parents = Parents::default();
        for list in &parent_ids {
            parents.push(list.iter().filter_map(|id| index.get(id).copied()));
        }
        let order = date_order(&times, &parents);
        let rows: HashMap<_, _> = order.iter().enumerate().map(|(row, &i)| (ids[i], row)).collect();
        let mut by_row: HashMap<usize, Vec<Label>> = HashMap::new();
        for (id, label) in labels {
            by_row.entry(rows[&id]).or_default().push(label);
        }
        for list in by_row.values_mut() {
            list.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        }
        Ok(History {
            graph: Graph::new(parents.reorder(&order)),
            head: head.map(|id| rows[&id]),
            ids: order.iter().map(|&i| ids[i]).collect(),
            labels: by_row,
        })
    }

    /// Reads one commit.
    pub fn commit(&self, id: ObjectId) -> Result<Commit> {
        let commit = self.gix.find_commit(id).map_err(Error::repository)?;
        let author = commit.author().map_err(Error::repository)?;
        Ok(Commit {
            message: commit.message_raw().map_err(Error::repository)?.to_string(),
            author: author.name.to_string(),
            email: author.email.to_string(),
            time: author.time().map_err(Error::repository)?,
        })
    }
}
