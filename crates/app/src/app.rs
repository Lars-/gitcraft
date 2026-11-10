//! The app's state and everything that changes it.
//!
//! The views never change the state themselves: they return [`Action`]s, which
//! [`Workspace::apply`] carries out after the frame is drawn.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use gitcraft_core::conflict::{Conflicts, Pick};
use gitcraft_core::diff::{self, FileDiff, Source};
use gitcraft_core::git::{Op, Step};
use gitcraft_core::repo::{Commit, History, Repo, Underway};
use gitcraft_core::status::{self, Change, FileChange, Status};
use gitcraft_core::undo::{self, Undo};
use gitcraft_core::watch::Watch;
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
    /// Resolve a conflicted file in the centre, conflict by conflict.
    Resolve(String),
    /// Keep a side of the conflict on screen, and go on to the next conflict without a pick.
    Pick(Pick),
    /// Show a conflict of the file being resolved.
    GoTo(usize),
    /// Put together an interactive rebase onto a commit, in the centre.
    Plan(String),
    /// Put together an interactive rebase that gives a commit a new message.
    Reword(ObjectId),
    /// Put together an interactive rebase that moves a commit to the place of another.
    Move(ObjectId, ObjectId),
    /// Set what the rebase does with a commit of the plan.
    Step(usize, Step),
    /// Move a commit of the plan to another place.
    Shift(usize, usize),
    /// Ask for a name first.
    Prompt(Prompt),
    /// Ask whether to go ahead first.
    Confirm(Confirm),
    /// Delete a branch, asking first when no other branch or tag has all of its commits.
    DeleteBranch(String),
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

/// An operation that throws work away, waiting for the user to go ahead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    /// What will be lost, as a question.
    pub question: String,
    /// The button that goes ahead.
    pub verb: &'static str,
    /// The operation.
    pub op: Op,
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
    /// The names of the remotes.
    pub remotes: Vec<String>,
    /// What undo and redo would do.
    pub undo: Undo,
    /// What stopped half way, if anything.
    pub underway: Option<Underway>,
}

impl Snapshot {
    fn load(repo: &Repo) -> Result<Self> {
        let status = Status::read(&repo.git)?;
        Ok(Self {
            history: repo.history()?,
            undo: undo::read(repo, &status)?,
            stashes: status::stashes(&repo.git)?,
            remotes: repo.git.remotes()?,
            underway: repo.underway(),
            status,
        })
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

/// A conflicted file being resolved in the centre, conflict by conflict.
#[derive(Debug)]
pub struct Merge {
    /// The file.
    pub path: String,
    /// Its conflicts, as git marked them.
    pub conflicts: Conflicts,
    /// What to keep of each conflict.
    pub picks: Vec<Option<Pick>>,
    /// The conflict on screen.
    pub at: usize,
    /// Whether to scroll to it.
    pub scroll: bool,
}

impl Merge {
    fn new(path: String, conflicts: Conflicts) -> Self {
        Self { path, picks: vec![None; conflicts.count()], conflicts, at: 0, scroll: true }
    }

    fn go_to(&mut self, at: usize) {
        (self.at, self.scroll) = (at, true);
    }

    /// Keeps `pick` of the conflict on screen, and goes on to the next conflict without a pick.
    fn pick(&mut self, pick: Pick) {
        self.picks[self.at] = Some(pick);
        let count = self.picks.len();
        if let Some(next) = (1..count).map(|step| (self.at + step) % count).find(|&i| self.picks[i].is_none()) {
            self.go_to(next);
        }
    }
}

/// An interactive rebase being put together in the centre.
#[derive(Debug)]
pub struct Plan {
    /// The commit to rebase onto.
    pub onto: String,
    /// The commits it takes along, as git listed them.
    pub listed: Vec<ObjectId>,
    /// Each commit with what to do with it, in the new order, newest first like the graph.
    pub steps: Vec<(Step, ObjectId)>,
    /// The new message of each commit that was to be reworded, also when it is not any more.
    pub messages: HashMap<ObjectId, String>,
    /// The commit whose new message gets the cursor.
    pub focus: Option<ObjectId>,
}

impl Plan {
    fn new(onto: String, listed: Vec<ObjectId>) -> Self {
        let steps = listed.iter().rev().map(|&id| (Step::Pick, id)).collect();
        Self { steps, onto, listed, messages: HashMap::new(), focus: None }
    }

    /// The rebase, or why it cannot start.
    pub fn op(&self) -> Result<Op, &'static str> {
        let first = self.steps.iter().rev().map(|&(step, _)| step).find(|&step| step != Step::Drop);
        if matches!(first, Some(Step::Squash | Step::Fixup)) {
            return Err("The oldest commit it keeps has nothing below it to meld into");
        }
        let rewords = self.steps.iter().filter(|&&(step, _)| step == Step::Reword);
        let messages: BTreeMap<_, _> = rewords.map(|(_, id)| (id.to_string(), self.messages[id].clone())).collect();
        if messages.values().any(|message| message.trim().is_empty()) {
            return Err("A commit to reword needs a message");
        }
        let todo = self.steps.iter().rev().map(|&(step, id)| (step, id.to_string())).collect();
        Ok(Op::RebaseInteractive { onto: self.onto.clone(), todo, messages })
    }
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

type Outcome = (Option<String>, Result<Snapshot>, Option<Result<Watch>>);

/// An operation running in the background, followed by a reload.
#[derive(Debug)]
struct Job {
    op: Option<Op>,
    outcome: Receiver<Outcome>,
    started: Instant,
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
    /// The conflicted file in the centre, if any.
    pub merge: Option<Merge>,
    /// The interactive rebase in the centre, if any.
    pub plan: Option<Plan>,
    /// The commit summary being written.
    pub summary: String,
    /// The rest of the commit message.
    pub description: String,
    /// Whether the next commit replaces the last one.
    pub amend: bool,
    /// A name being asked for.
    pub prompt: Option<(Prompt, String)>,
    /// An operation waiting for the user to go ahead.
    pub confirm: Option<Confirm>,
    /// The last thing that went wrong.
    pub error: Option<String>,
    /// A graph row to scroll to.
    pub scroll_to: Option<usize>,
    /// The graph rows on screen.
    pub visible: Range<usize>,
    job: Option<Job>,
    /// What the user asked for while a job ran, to run after it, in order.
    queue: Vec<Op>,
    /// Set when something changed since the last reload began.
    changed: Arc<AtomicBool>,
    /// Keeps `changed` up to date.
    watch: Option<Watch>,
    /// When the next reload may start by itself.
    quiet_until: Instant,
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
            merge: None,
            plan: None,
            summary: String::new(),
            description: String::new(),
            amend: false,
            prompt: None,
            confirm: None,
            error: None,
            scroll_to: None,
            visible: 0..0,
            job: None,
            queue: Vec::new(),
            changed: Arc::default(),
            watch: None,
            quiet_until: Instant::now(),
        };
        workspace.spawn(ctx, None, true);
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
                Ok(diff) => {
                    (self.merge, self.plan) = (None, None);
                    self.open_file = Some(OpenFile { source, diff, lines: BTreeSet::new() });
                }
                Err(error) => self.error = Some(error.to_string()),
            },
            Action::Resolve(path) => match Conflicts::read(&self.repo.git, &path) {
                Ok(conflicts) => {
                    (self.merge, self.open_file, self.plan) = (Some(Merge::new(path, conflicts)), None, None);
                }
                Err(error) => self.error = Some(error.to_string()),
            },
            Action::Pick(pick) => self.merge.iter_mut().for_each(|merge| merge.pick(pick)),
            Action::GoTo(at) => self.merge.iter_mut().for_each(|merge| merge.go_to(at)),
            Action::Plan(onto) => {
                self.plan(onto);
            }
            Action::Reword(id) => {
                if let Some(&[row]) = self.plan_from(id, &[id]).as_deref() {
                    self.apply(ctx, Action::Step(row, Step::Reword));
                    self.plan.iter_mut().for_each(|plan| plan.focus = Some(id));
                }
            }
            Action::Move(from, to) => {
                let history = &self.snapshot.history;
                let oldest = if history.row(from) > history.row(to) { from } else { to };
                if let Some(&[from, to]) = self.plan_from(oldest, &[from, to]).as_deref() {
                    self.apply(ctx, Action::Shift(from, to));
                }
            }
            Action::Step(row, step) => {
                let Some(plan) = &mut self.plan else { return };
                let id = plan.steps[row].1;
                // A new message starts from the one it has.
                if step == Step::Reword && !plan.messages.contains_key(&id) {
                    match self.commits.get(&self.repo, id) {
                        Ok(commit) => {
                            plan.messages.insert(id, commit.message.trim_end().to_owned());
                        }
                        Err(error) => {
                            self.error = Some(error.to_string());
                            return;
                        }
                    }
                }
                plan.steps[row].0 = step;
            }
            Action::Shift(from, to) => self.plan.iter_mut().for_each(|plan| {
                let step = plan.steps.remove(from);
                plan.steps.insert(to, step);
            }),
            Action::CloseFile => (self.open_file, self.merge, self.plan) = (None, None, None),
            Action::ToggleLine(hunk, line) => {
                if let Some(file) = &mut self.open_file
                    && !file.lines.remove(&(hunk, line))
                {
                    file.lines.insert((hunk, line));
                }
            }
            Action::Prompt(prompt) => self.prompt = Some((prompt, String::new())),
            Action::Confirm(confirm) => self.confirm = Some(confirm),
            Action::DeleteBranch(name) => match self.repo.git.only_on(&name) {
                Ok(0) => self.start(ctx, Some(Op::DeleteBranch(name))),
                Ok(lost) => {
                    let lost = crate::theme::count(lost, "commit");
                    let question = format!("Delete {name} and the {lost} on no other branch or tag?");
                    self.confirm = Some(Confirm { question, verb: "Delete", op: Op::DeleteBranch(name) });
                }
                Err(error) => self.error = Some(error.to_string()),
            },
            Action::OpenRepo(_) | Action::ShowTab(_) | Action::CloseTab(_) => unreachable!("the app handles tabs"),
        }
    }

    /// Puts together an interactive rebase onto `onto`, in the centre.
    fn plan(&mut self, onto: String) -> Option<&Plan> {
        match self.repo.git.rebase_plan(&onto) {
            Ok(listed) if listed.is_empty() => self.error = Some(format!("Nothing to rebase onto {onto:.7}")),
            Ok(listed) => {
                (self.open_file, self.merge) = (None, None);
                return Some(self.plan.insert(Plan::new(onto, listed)));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        None
    }

    /// Puts together an interactive rebase onto the parent of `oldest`, and gives the rows of
    /// `ids` in it: only when it takes them all along.
    fn plan_from(&mut self, oldest: ObjectId, ids: &[ObjectId]) -> Option<Vec<usize>> {
        let history = &self.snapshot.history;
        let parents: Vec<_> = history.row(oldest).into_iter().flat_map(|row| history.graph.parents(row)).collect();
        // The first commit has nothing to rebase onto, and a rebase leaves out merges.
        let onto = match parents[..] {
            [parent] => Some(history.ids[parent].to_string()),
            _ => None,
        };
        let rows = onto
            .and_then(|onto| self.plan(onto))
            .and_then(|plan| ids.iter().map(|&id| plan.steps.iter().position(|step| step.1 == id)).collect());
        if rows.is_none() {
            self.plan = None;
            self.error = Some(
                "Only commits of the current branch can be moved or reworded, apart from merges and the first commit"
                    .to_owned(),
            );
        }
        rows
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

    /// Reloads when something changed, leaving at least as much time between two reloads as the
    /// last one took, so that a build or a long checkout never keeps the window busy.
    pub fn reload_if_changed(&mut self, ctx: &egui::Context) {
        if self.job.is_some() || !self.changed.load(Ordering::Relaxed) {
            return;
        }
        match self.quiet_until.checked_duration_since(Instant::now()) {
            Some(wait) => ctx.request_repaint_after(wait),
            None => self.start(ctx, None),
        }
    }

    fn start(&mut self, ctx: &egui::Context, op: Option<Op>) {
        if self.job.is_some() {
            // A change waits its turn; a reload follows the running job anyway.
            self.queue.extend(op);
        } else {
            self.spawn(ctx, op, false);
        }
    }

    /// Runs `op` and reloads, in the background. The first job also starts watching, which takes
    /// a while in a large working tree.
    fn spawn(&mut self, ctx: &egui::Context, op: Option<Op>, watch: bool) {
        let (send, outcome) = mpsc::channel();
        let (repo, ctx, job_op, changed) = (self.repo.clone(), ctx.clone(), op.clone(), self.changed.clone());
        std::thread::spawn(move || {
            let watch = watch.then(|| {
                let (changed, ctx) = (changed.clone(), ctx.clone());
                repo.watch(move || {
                    changed.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                })
            });
            let failed = job_op.and_then(|op| repo.git.apply(&op).err()).map(|error| error.to_string());
            // The reload sees every change made until here.
            changed.store(false, Ordering::Relaxed);
            // Sending fails only when the repository was closed in the meantime.
            let _ = send.send((failed, Snapshot::load(&repo), watch));
            ctx.request_repaint();
        });
        self.job = Some(Job { op, outcome, started: Instant::now() });
    }

    /// Takes in the result of the background job, once it is there, and starts the next.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let (failed, snapshot, watch) = match job.outcome.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => (Some("The background job stopped unexpectedly".to_owned()), None, None),
            Ok((failed, snapshot, watch)) => (failed, Some(snapshot), watch),
        };
        let job = self.job.take().expect("a job is running");
        self.quiet_until = Instant::now() + job.started.elapsed().max(Duration::from_millis(100));
        if !self.queue.is_empty() {
            let next = self.queue.remove(0);
            self.spawn(ctx, Some(next), false);
        }
        let op = job.op;
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
        match watch {
            Some(Ok(watch)) => self.watch = Some(watch),
            Some(Err(error)) => self.error = Some(error.to_string()),
            None => {}
        }
        // Nothing selected, changes that are gone, or a commit that left the graph: show HEAD.
        if self.selected_row().is_none() {
            let head = self.snapshot.history.head.map(|row| self.snapshot.history.ids[row]);
            match head {
                Some(id) => self.select(Selection::Commit(id)),
                None => self.selection = None,
            }
        }
        if let Some(merge) = &mut self.merge {
            // A file without conflicts left closes; one changed outside starts again.
            let entries = &self.snapshot.status.entries;
            let conflicted =
                entries.iter().any(|e| e.path == merge.path && matches!(e.unstaged, Some(Change::Conflicted { .. })));
            match conflicted.then(|| Conflicts::read(&self.repo.git, &merge.path)) {
                None => self.merge = None,
                Some(Ok(conflicts)) if conflicts == merge.conflicts => {}
                Some(Ok(conflicts)) => *merge = Merge::new(std::mem::take(&mut merge.path), conflicts),
                Some(Err(error)) => (self.merge, self.error) = (None, Some(error.to_string())),
            }
        }
        // A rebase that started is done with its plan, also when it stopped at a conflict.
        if matches!(op, Some(Op::RebaseInteractive { .. }))
            && (self.error.is_none() || self.snapshot.underway.is_some())
        {
            self.plan = None;
        }
        if let Some(plan) = &mut self.plan {
            // Commits changed outside start the plan again, so that the rebase never leaves one out.
            match self.repo.git.rebase_plan(&plan.onto) {
                Ok(listed) if listed == plan.listed => {}
                Ok(listed) if listed.is_empty() => self.plan = None,
                Ok(listed) => *plan = Plan::new(std::mem::take(&mut plan.onto), listed),
                Err(error) => (self.plan, self.error) = (None, Some(error.to_string())),
            }
        }
        if let Some(file) = &mut self.open_file {
            // A file in the working tree without changes left, or gone altogether, closes.
            let entries = &self.snapshot.status.entries;
            if !matches!(file.source, Source::Commit { .. }) && !entries.iter().any(|e| e.path == file.source.path()) {
                self.open_file = None;
                return;
            }
            // The picked lines stay picked as long as the diff stays the same.
            match diff::read(&self.repo.git, &file.source) {
                Ok(diff) if diff == file.diff => {}
                Ok(diff) => (file.diff, file.lines) = (diff, BTreeSet::new()),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
}
