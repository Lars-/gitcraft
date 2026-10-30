//! The app's state and everything that changes it.
//!
//! The views never change the state themselves: they return [`Action`]s, which
//! [`Workspace::apply`] carries out after the frame is drawn.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;
use gitcraft_core::diff::{self, FileDiff, Source};
use gitcraft_core::git::Op;
use gitcraft_core::repo::{Commit, History, Repo};
use gitcraft_core::status::{self, FileChange, Status};
use gitcraft_core::{ObjectId, Result};

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Change the repository, then reload it.
    Run(Op),
    /// Reload the repository.
    Refresh,
    /// Show something in the details panel.
    Select(Selection),
    /// Select something and scroll the graph to it.
    Reveal(Selection),
    /// Show the diff of a file instead of the graph.
    Open(Source),
    /// Back to the graph.
    CloseFile,
    /// Pick or unpick a line of the open diff for staging.
    ToggleLine(usize, usize),
    /// Ask for a name first.
    Prompt(Prompt),
    /// Open a repository in a tab of its own, or show its tab if it is open.
    OpenRepo(PathBuf),
    /// Show a tab, or the start screen for `None`.
    ShowTab(Option<usize>),
    /// Close a tab.
    CloseTab(usize),
}

/// What the details panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// The uncommitted changes.
    Wip,
    /// A commit.
    Commit(ObjectId),
}

/// An operation that needs a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// A new branch at a commit.
    Branch {
        /// The commit.
        at: String,
    },
    /// A new tag at a commit.
    Tag {
        /// The commit.
        at: String,
    },
}

impl Prompt {
    /// The operation, once the name is known.
    pub fn op(&self, name: String) -> Op {
        match self {
            Self::Branch { at } => Op::Branch { name, at: at.clone() },
            Self::Tag { at } => Op::Tag { name, at: at.clone() },
        }
    }
}

/// Everything read in the background after a change.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// The graph.
    pub history: History,
    /// The working tree.
    pub status: Status,
    /// The stash messages, newest first.
    pub stashes: Vec<String>,
}

impl Snapshot {
    fn load(repo: &Repo) -> Result<Self> {
        Ok(Self { history: repo.history()?, status: Status::read(&repo.git)?, stashes: status::stashes(&repo.git)? })
    }
}

/// A file whose diff fills the centre, with the lines picked for staging.
#[derive(Debug)]
pub struct OpenFile {
    /// Which diff it is.
    pub source: Source,
    /// The diff.
    pub diff: FileDiff,
    /// The picked lines, as hunk and line index.
    pub lines: BTreeSet<(usize, usize)>,
}

/// Commits read so far, so scrolling back costs nothing.
#[derive(Debug, Default)]
pub struct Commits(HashMap<ObjectId, Commit>);

impl Commits {
    /// Reads a commit, or takes it from the cache.
    pub fn get(&mut self, repo: &Repo, id: ObjectId) -> Result<&Commit> {
        if self.0.len() > 100_000 {
            self.0.clear();
        }
        Ok(match self.0.entry(id) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(repo.commit(id)?),
        })
    }
}

type Outcome = (Option<String>, Result<Snapshot>);

/// An operation running in the background, followed by a reload.
#[derive(Debug)]
struct Job {
    op: Option<Op>,
    outcome: Receiver<Outcome>,
}

/// An open repository.
#[derive(Debug)]
pub struct Workspace {
    /// The repository.
    pub repo: Repo,
    /// What was last read from it.
    pub snapshot: Snapshot,
    /// Commits read for display.
    pub commits: Commits,
    /// What the details panel shows.
    pub selection: Option<Selection>,
    /// The files changed by the selected commit.
    pub files: Option<(ObjectId, Vec<FileChange>)>,
    /// The diff in the centre, if any.
    pub open_file: Option<OpenFile>,
    /// The commit summary being written.
    pub summary: String,
    /// The rest of the commit message.
    pub description: String,
    /// Whether the next commit replaces the last one.
    pub amend: bool,
    /// A name being asked for.
    pub prompt: Option<(Prompt, String)>,
    /// The last thing that went wrong.
    pub error: Option<String>,
    /// A graph row to scroll to.
    pub scroll_to: Option<usize>,
    /// The graph rows on screen.
    pub visible: Range<usize>,
    job: Option<Job>,
}

impl Workspace {
    /// Opens `repo` and starts reading it.
    pub fn new(ctx: &egui::Context, repo: Repo) -> Self {
        let mut workspace = Self {
            repo,
            snapshot: Snapshot::default(),
            commits: Commits::default(),
            selection: None,
            files: None,
            open_file: None,
            summary: String::new(),
            description: String::new(),
            amend: false,
            prompt: None,
            error: None,
            scroll_to: None,
            visible: 0..0,
            job: None,
        };
        workspace.start(ctx, None);
        workspace
    }

    /// What is running in the background, if anything.
    pub fn busy(&self) -> Option<String> {
        self.job.as_ref().map(|job| match &job.op {
            Some(op) => format!("git {}", op.args()[0]),
            None => "Loading".to_owned(),
        })
    }

    /// The row of the selection in the graph, counting the uncommitted changes as row 0.
    pub fn selected_row(&self) -> Option<usize> {
        let wip = usize::from(!self.snapshot.status.is_clean());
        match self.selection? {
            Selection::Wip => (wip == 1).then_some(0),
            Selection::Commit(id) => self.snapshot.history.row(id).map(|row| row + wip),
        }
    }

    /// What is in graph row `row`, counting the uncommitted changes as row 0.
    pub fn row_selection(&self, row: usize) -> Option<Selection> {
        let wip = usize::from(!self.snapshot.status.is_clean());
        if row < wip {
            return Some(Selection::Wip);
        }
        self.snapshot.history.ids.get(row - wip).map(|&id| Selection::Commit(id))
    }

    /// Carries out an action.
    pub fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Run(op) => self.start(ctx, Some(op)),
            Action::Refresh => self.start(ctx, None),
            Action::Select(selection) => self.select(selection),
            Action::Reveal(selection) => {
                self.select(selection);
                self.scroll_to = self.selected_row();
            }
            Action::Open(source) => match diff::read(&self.repo.git, &source) {
                Ok(diff) => self.open_file = Some(OpenFile { source, diff, lines: BTreeSet::new() }),
                Err(error) => self.error = Some(error.to_string()),
            },
            Action::CloseFile => self.open_file = None,
            Action::ToggleLine(hunk, line) => {
                if let Some(file) = &mut self.open_file
                    && !file.lines.remove(&(hunk, line))
                {
                    file.lines.insert((hunk, line));
                }
            }
            Action::Prompt(prompt) => self.prompt = Some((prompt, String::new())),
            Action::OpenRepo(_) | Action::ShowTab(_) | Action::CloseTab(_) => unreachable!("the app handles tabs"),
        }
    }

    fn select(&mut self, selection: Selection) {
        self.selection = Some(selection);
        let Selection::Commit(id) = selection else { return };
        if self.files.as_ref().is_some_and(|(files_of, _)| *files_of == id) {
            return;
        }
        match status::commit_files(&self.repo.git, &id.to_string()) {
            Ok(files) => self.files = Some((id, files)),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn start(&mut self, ctx: &egui::Context, op: Option<Op>) {
        if self.job.is_some() {
            return;
        }
        let (send, outcome) = mpsc::channel();
        let (repo, ctx, job_op) = (self.repo.clone(), ctx.clone(), op.clone());
        std::thread::spawn(move || {
            let failed = job_op.and_then(|op| repo.git.apply(&op).err()).map(|error| error.to_string());
            // Sending fails only when the repository was closed in the meantime.
            let _ = send.send((failed, Snapshot::load(&repo)));
            ctx.request_repaint();
        });
        self.job = Some(Job { op, outcome });
    }

    /// Takes in the result of the background job, once it is there.
    pub fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let (failed, snapshot) = match job.outcome.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => (Some("The background job stopped unexpectedly".to_owned()), None),
            Ok((failed, snapshot)) => (failed, Some(snapshot)),
        };
        let op = self.job.take().and_then(|job| job.op);
        if failed.is_none() && matches!(op, Some(Op::Commit { .. })) {
            self.summary.clear();
            self.description.clear();
            self.amend = false;
        }
        self.error = failed;
        match snapshot {
            Some(Ok(snapshot)) => self.snapshot = snapshot,
            Some(Err(error)) => self.error = Some(error.to_string()),
            None => {}
        }
        if self.selection == Some(Selection::Wip) && self.snapshot.status.is_clean() || self.selection.is_none() {
            let head = self.snapshot.history.head.map(|row| self.snapshot.history.ids[row]);
            match head {
                Some(id) => self.select(Selection::Commit(id)),
                None => self.selection = None,
            }
        }
        if let Some(file) = &mut self.open_file {
            match diff::read(&self.repo.git, &file.source) {
                Ok(diff) => (file.diff, file.lines) = (diff, BTreeSet::new()),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
}
