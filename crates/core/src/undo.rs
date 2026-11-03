//! Undo and redo, read from the reflog of `HEAD`.
//!
//! Every action that moves `HEAD` adds a line to its reflog, also when it ran on the command line.
//! Undo moves `HEAD` back to where the last action found it, with a checkout or a reset, and starts
//! its own line with `undo`; redo moves it forward again and starts its line with `redo`. Read from
//! old to new, an action goes on a stack, an undo moves the top of that stack to a second one and a
//! redo moves it back: the two tops are what undo and redo do next.

use std::collections::HashSet;

use gix::bstr::ByteSlice;

use crate::git::{Op, ResetMode};
use crate::repo::Repo;
use crate::status::Status;
use crate::{Error, ObjectId, Result};

/// What undo and redo would do now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Undo {
    /// Takes back the last action.
    pub last: Option<Move>,
    /// Does the last undone action again.
    pub undone: Option<Move>,
}

/// An undo or a redo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// The action it takes back or does again, as the reflog says, like `commit: Fix the build`.
    pub what: String,
    /// The checkout or reset that does it.
    pub op: Op,
}

/// The actions that can be undone, oldest first, and those that can be redone, last undone last.
#[derive(Debug, Default)]
struct Stacks {
    done: Vec<Action>,
    undone: Vec<Action>,
}

impl Stacks {
    /// A new action: what was undone before it cannot be redone any more.
    fn act(&mut self, action: Action) {
        self.done.push(action);
        self.undone.clear();
    }
}

/// An action that moved `HEAD` from `old` to `new`.
#[derive(Debug)]
struct Action {
    what: String,
    old: ObjectId,
    new: ObjectId,
    how: How,
}

#[derive(Debug)]
enum How {
    /// From one branch or commit to another.
    Checkout { from: String, to: String },
    /// A move of the current branch. The reflog does not say how a reset left the index and the
    /// working tree, so that is `None` until an undo found out.
    Reset(Option<ResetMode>),
}

/// Reads what undo and redo would do, given the `status` of the working tree.
pub fn read(repo: &Repo, status: &Status) -> Result<Undo> {
    let mut log = repo.gix.head().map_err(Error::repository)?.log_iter();
    let Some(lines) = log.all().map_err(Error::repository)? else { return Ok(Undo::default()) };
    let mut stacks = Stacks::default();
    let mut rebase: Option<(String, ObjectId)> = None;
    let mut at = None;
    for line in lines {
        let line = line.map_err(Error::repository)?;
        let (old, new, message) = (line.previous_oid(), line.new_oid(), line.message.to_str_lossy());
        let action = message.split_once(':').map_or(&*message, |(action, _)| action);
        // A gap in the log, or a new start, leaves nothing before it to undo.
        if old.is_null() || at.is_some_and(|at| at != old) {
            (stacks, rebase) = (Stacks::default(), None);
        }
        at = Some(new);
        if old.is_null() {
            continue;
        }
        // A rebase adds a line per commit, and counts as one action once it finished.
        if let Some((name, start)) = rebase.take() {
            if let Some(part) = action.strip_prefix(name.as_str()).and_then(|part| part.strip_prefix(" (")) {
                match part {
                    "finish)" => {
                        stacks.act(Action { what: name, old: start, new, how: How::Reset(Some(ResetMode::Keep)) });
                    }
                    "abort)" => {}
                    _ => rebase = Some((name, start)),
                }
                continue;
            }
            // Something else happened in the middle of it.
            stacks = Stacks::default();
        }
        if let Some(name) = action.strip_suffix(" (start)") {
            rebase = Some((name.to_owned(), old));
            continue;
        }
        let mut words = action.split(' ');
        match (words.next(), words.next().and_then(ResetMode::from_flag)) {
            (Some("undo"), mode) => match stacks.done.pop() {
                Some(mut undid) if (undid.new, undid.old) == (old, new) => {
                    if let How::Reset(known) = &mut undid.how {
                        *known = known.or(mode);
                    }
                    stacks.undone.push(undid);
                }
                _ => stacks = Stacks::default(),
            },
            (Some("redo"), _) => match stacks.undone.pop() {
                Some(redid) if (redid.old, redid.new) == (old, new) => stacks.done.push(redid),
                _ => stacks = Stacks::default(),
            },
            _ => {
                let how = if action == "checkout" {
                    let Some((from, to)) =
                        message.strip_prefix("checkout: moving from ").and_then(|m| m.split_once(" to "))
                    else {
                        stacks = Stacks::default();
                        continue;
                    };
                    How::Checkout { from: from.to_owned(), to: to.to_owned() }
                } else if old == new {
                    // It moved nothing, like the reset a stash does.
                    continue;
                } else {
                    How::Reset(match action {
                        "commit" | "commit (amend)" => Some(ResetMode::Soft),
                        "reset" => None,
                        _ => Some(ResetMode::Keep),
                    })
                };
                stacks.act(Action { what: message.clone().into_owned(), old, new, how });
            }
        }
    }
    // A rebase under way, or a move the log does not know of, leaves nothing to undo.
    if rebase.is_some() || at != repo.gix.head_id().ok().map(gix::Id::detach) {
        return Ok(Undo::default());
    }
    Ok(Undo {
        last: stacks.done.pop().map(|action| action.step(false, repo, status)).transpose()?,
        undone: stacks.undone.pop().map(|action| action.step(true, repo, status)).transpose()?,
    })
}

impl Action {
    /// Back to where it started, or forward again to where it ended.
    fn step(self, forward: bool, repo: &Repo, status: &Status) -> Result<Move> {
        let (verb, id) = if forward { ("redo", self.new) } else { ("undo", self.old) };
        let (mark, op) = match self.how {
            How::Checkout { from, to } => {
                let name = if forward { to } else { from };
                // The branch, while it is there; otherwise the commit.
                let branch = repo.gix.find_reference(format!("refs/heads/{name}").as_str()).is_ok();
                (verb.to_owned(), Op::Checkout(if branch { name } else { id.to_string() }))
            }
            How::Reset(mode) => {
                let mode = match mode {
                    Some(mode) => mode,
                    None => left_by_reset(repo, status, id)?,
                };
                (format!("{verb} {}", mode.flag()), Op::Reset { to: id.to_string(), mode })
            }
        };
        Ok(Move { what: self.what, op: Op::Undo { mark, op: Box::new(op) } })
    }
}

/// How a reset left the files it changed, to undo it the same way: a soft reset leaves their
/// changes staged, a mixed one in the working tree, and a hard one nowhere.
fn left_by_reset(repo: &Repo, status: &Status, to: ObjectId) -> Result<ResetMode> {
    let changed = repo.git.run(&["diff-tree", "-r", "--name-only", "-z", "HEAD", &to.to_string()])?;
    let changed: HashSet<_> = changed.split('\0').collect();
    let touched: Vec<_> = status.entries.iter().filter(|entry| changed.contains(entry.path.as_str())).collect();
    Ok(if touched.iter().any(|entry| entry.staged.is_some()) {
        ResetMode::Soft
    } else if touched.is_empty() {
        ResetMode::Keep
    } else {
        ResetMode::Mixed
    })
}
