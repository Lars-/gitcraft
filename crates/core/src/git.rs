//! Running the user's `git`, and the changes GitCraft makes with it.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::{Error, Result};

/// A repository directory to run `git` in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    dir: PathBuf,
}

impl Git {
    /// Runs `git` in `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory `git` runs in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Runs `git` with `args` and returns what it printed.
    pub fn run(&self, args: &[impl AsRef<OsStr>]) -> Result<String> {
        self.run_with(args, None, &[0])
    }

    /// Carries out `op`.
    pub fn apply(&self, op: &Op) -> Result<String> {
        self.run_with(&op.args(), op.input(), &[0])
    }

    /// Runs `git` with `input` on standard input, accepting the exit codes in `ok`.
    pub(crate) fn run_with(&self, args: &[impl AsRef<OsStr>], input: Option<&[u8]>, ok: &[i32]) -> Result<String> {
        let output = self.output(args, input)?;
        if output.status.code().is_some_and(|code| ok.contains(&code)) {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let args: Vec<_> = args.iter().map(|a| a.as_ref().to_string_lossy()).collect();
        Err(Error::Git { args: args.join(" "), message: String::from_utf8_lossy(&output.stderr).trim().to_owned() })
    }

    fn output(&self, args: &[impl AsRef<OsStr>], input: Option<&[u8]>) -> Result<Output> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
            .args(args)
            // A graphical client must never wait for a terminal or an editor.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        std::thread::scope(|scope| {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                // A failed write means git stopped reading; its exit status says why.
                scope.spawn(move || stdin.write_all(input));
            }
            child.wait_with_output()
        })
        .map_err(Error::from)
    }
}

/// How far [`Op::Reset`] goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Keep the index and the working tree.
    Soft,
    /// Keep the working tree.
    Mixed,
    /// Discard everything.
    Hard,
}

impl ResetMode {
    fn flag(self) -> &'static str {
        match self {
            Self::Soft => "--soft",
            Self::Mixed => "--mixed",
            Self::Hard => "--hard",
        }
    }
}

fn strs(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

fn flag(on: bool, flag: &str) -> Vec<&str> {
    on.then_some(flag).into_iter().collect()
}

/// A change to the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Add files to the index.
    Stage(Vec<String>),
    /// Take files out of the index.
    Unstage(Vec<String>),
    /// Throw away unstaged changes to tracked files.
    Discard(Vec<String>),
    /// Delete untracked files.
    DeleteUntracked(Vec<String>),
    /// Apply a patch to the index, or take it out with `reverse`.
    ApplyToIndex {
        /// The patch, as built by [`crate::diff::FileDiff::patch`].
        patch: String,
        /// Take the patch out of the index instead.
        reverse: bool,
    },
    /// Commit the index.
    Commit {
        /// The full message.
        message: String,
        /// Replace the last commit instead.
        amend: bool,
    },
    /// Switch to a branch, or to a commit or remote branch without one.
    Checkout(String),
    /// Create a branch at a commit and switch to it.
    Branch {
        /// The new branch.
        name: String,
        /// Where it starts.
        at: String,
    },
    /// Delete a merged branch.
    DeleteBranch(String),
    /// Merge into the current branch.
    Merge(String),
    /// Rebase the current branch onto another.
    Rebase(String),
    /// Copy a commit onto the current branch.
    CherryPick(String),
    /// Add a commit that undoes another.
    Revert(String),
    /// Move the current branch.
    Reset {
        /// Where to.
        to: String,
        /// What to keep.
        mode: ResetMode,
    },
    /// Tag a commit.
    Tag {
        /// The tag.
        name: String,
        /// The commit.
        at: String,
    },
    /// Delete a tag.
    DeleteTag(String),
    /// Stash all changes, untracked files included.
    Stash(String),
    /// Apply a stash and drop it.
    StashPop(usize),
    /// Apply a stash and keep it.
    StashApply(usize),
    /// Drop a stash.
    StashDrop(usize),
    /// Fetch every remote and prune what is gone.
    Fetch,
    /// Fetch and integrate the upstream branch.
    Pull {
        /// Rebase onto it instead of merging.
        rebase: bool,
    },
    /// Push the current branch.
    Push {
        /// Set this remote as the upstream first.
        upstream: Option<String>,
        /// Overwrite the remote branch, unless someone else pushed to it.
        force: bool,
    },
}

impl Op {
    /// The arguments for `git`.
    pub fn args(&self) -> Vec<String> {
        let stash = match self {
            Self::StashPop(n) | Self::StashApply(n) | Self::StashDrop(n) => {
                format!("stash@{{{n}}}")
            }
            _ => String::new(),
        };
        let (command, rest): (&[&str], Vec<&str>) = match self {
            Self::Stage(paths) => (&["add", "--"], strs(paths)),
            Self::Unstage(paths) => (&["reset", "-q", "--"], strs(paths)),
            Self::Discard(paths) => (&["checkout", "-q", "--"], strs(paths)),
            Self::DeleteUntracked(paths) => (&["clean", "-f", "-q", "--"], strs(paths)),
            Self::ApplyToIndex { reverse, .. } => {
                (&["apply", "--cached", "--recount"], [flag(*reverse, "--reverse"), vec!["-"]].concat())
            }
            Self::Commit { amend, .. } => (&["commit", "-q", "--cleanup=strip", "-F", "-"], flag(*amend, "--amend")),
            Self::Checkout(target) => (&["checkout", "-q"], vec![target]),
            Self::Branch { name, at } => (&["checkout", "-q", "-b"], vec![name, at]),
            Self::DeleteBranch(name) => (&["branch", "-d"], vec![name]),
            Self::Merge(target) => (&["merge", "--no-edit"], vec![target]),
            Self::Rebase(onto) => (&["rebase"], vec![onto]),
            Self::CherryPick(commit) => (&["cherry-pick"], vec![commit]),
            Self::Revert(commit) => (&["revert", "--no-edit"], vec![commit]),
            Self::Reset { to, mode } => (&["reset", "-q"], vec![mode.flag(), to]),
            Self::Tag { name, at } => (&["tag"], vec![name, at]),
            Self::DeleteTag(name) => (&["tag", "-d"], vec![name]),
            Self::Stash(message) => (&["stash", "push", "-q", "--include-untracked", "-m"], vec![message]),
            Self::StashPop(_) => (&["stash", "pop", "-q"], vec![&stash]),
            Self::StashApply(_) => (&["stash", "apply", "-q"], vec![&stash]),
            Self::StashDrop(_) => (&["stash", "drop", "-q"], vec![&stash]),
            Self::Fetch => (&["fetch", "--all", "--prune", "-q"], vec![]),
            Self::Pull { rebase } => (&["pull", "-q"], vec![if *rebase { "--rebase" } else { "--no-rebase" }]),
            Self::Push { upstream, force } => {
                let mut rest = flag(*force, "--force-with-lease");
                if let Some(remote) = upstream {
                    rest.extend(["-u", remote, "HEAD"]);
                }
                (&["push", "-q"], rest)
            }
        };
        command.iter().copied().chain(rest).map(str::to_owned).collect()
    }

    /// What `git` reads from standard input.
    pub fn input(&self) -> Option<&[u8]> {
        match self {
            Self::Commit { message, .. } => Some(message.as_bytes()),
            Self::ApplyToIndex { patch, .. } => Some(patch.as_bytes()),
            _ => None,
        }
    }
}
