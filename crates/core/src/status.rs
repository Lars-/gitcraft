//! What changed: in the working tree (`git status`) and in a commit.

use crate::git::Git;
use crate::{Error, Result};

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The file is new.
    Added,
    /// The content changed.
    Modified,
    /// The file is gone.
    Deleted,
    /// The file moved, maybe with changes.
    Renamed,
    /// The file was copied, maybe with changes.
    Copied,
    /// The file became a link, or the other way around.
    TypeChanged,
    /// Git does not track the file yet.
    Untracked,
    /// A merge left conflicts in the file.
    Conflicted,
}

impl Change {
    fn from_code(code: u8) -> Option<Self> {
        match code {
            b'A' => Some(Self::Added),
            b'M' => Some(Self::Modified),
            b'D' => Some(Self::Deleted),
            b'R' => Some(Self::Renamed),
            b'C' => Some(Self::Copied),
            b'T' => Some(Self::TypeChanged),
            _ => None,
        }
    }
}

/// A changed file in the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The path, relative to the repository root.
    pub path: String,
    /// Where a renamed or copied file came from.
    pub from: Option<String>,
    /// The change in the index, ready to commit.
    pub staged: Option<Change>,
    /// The change in the working tree, not staged.
    pub unstaged: Option<Change>,
}

/// The state of the working tree and the current branch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// The current branch, or `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// The branch it tracks.
    pub upstream: Option<String>,
    /// Commits to push.
    pub ahead: u32,
    /// Commits to pull.
    pub behind: u32,
    /// Every changed file.
    pub entries: Vec<Entry>,
}

impl Status {
    /// Reads the status of the repository.
    pub fn read(git: &Git) -> Result<Self> {
        Self::parse(&git.run(&["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"])?)
    }

    /// Whether nothing changed.
    pub fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }

    /// Parses `git status --porcelain=v2 -z --branch`.
    pub fn parse(output: &str) -> Result<Self> {
        let mut status = Self::default();
        let mut fields = output.split('\0').filter(|f| !f.is_empty());
        while let Some(record) = fields.next() {
            let bad = || Error::Parse(record.to_owned());
            let (kind, rest) = record.split_once(' ').ok_or_else(bad)?;
            let xy = |code: &str| {
                let code = code.as_bytes();
                (code.first().copied().and_then(Change::from_code), code.get(1).copied().and_then(Change::from_code))
            };
            let entry = match kind {
                "#" => {
                    status.header(rest).ok_or_else(bad)?;
                    continue;
                }
                "1" => {
                    let parts: Vec<_> = rest.splitn(8, ' ').collect();
                    entry(parts.get(7).ok_or_else(bad)?, None, xy(parts[0]))
                }
                "2" => {
                    let parts: Vec<_> = rest.splitn(9, ' ').collect();
                    let path = parts.get(8).ok_or_else(bad)?;
                    entry(path, Some(fields.next().ok_or_else(bad)?), xy(parts[0]))
                }
                "u" => entry(rest.splitn(10, ' ').nth(9).ok_or_else(bad)?, None, (None, Some(Change::Conflicted))),
                "?" => entry(rest, None, (None, Some(Change::Untracked))),
                "!" => continue,
                _ => return Err(bad()),
            };
            status.entries.push(entry);
        }
        Ok(status)
    }

    fn header(&mut self, line: &str) -> Option<()> {
        match line.split_once(' ')? {
            ("branch.head", name) => self.branch = (name != "(detached)").then(|| name.to_owned()),
            ("branch.upstream", name) => self.upstream = Some(name.to_owned()),
            ("branch.ab", counts) => {
                let (ahead, behind) = counts.split_once(' ')?;
                self.ahead = ahead.strip_prefix('+')?.parse().ok()?;
                self.behind = behind.strip_prefix('-')?.parse().ok()?;
            }
            _ => {}
        }
        Some(())
    }
}

fn entry(path: &str, from: Option<&str>, (staged, unstaged): (Option<Change>, Option<Change>)) -> Entry {
    Entry { path: path.to_owned(), from: from.map(str::to_owned), staged, unstaged }
}

/// The message of every stash, newest first; the position is the stash number.
pub fn stashes(git: &Git) -> Result<Vec<String>> {
    Ok(git.run(&["stash", "list", "--format=%gs"])?.lines().map(str::to_owned).collect())
}

/// A file changed by a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// The path after the commit.
    pub path: String,
    /// Where a renamed or copied file came from.
    pub from: Option<String>,
    /// How it changed.
    pub change: Change,
}

/// The files `commit` changed, compared with its first parent.
pub fn commit_files(git: &Git, commit: &str) -> Result<Vec<FileChange>> {
    parse_name_status(&git.run(&[
        "show",
        "--format=",
        "--name-status",
        "-z",
        "-M",
        "--diff-merges=first-parent",
        commit,
    ])?)
}

/// Parses `--name-status -z`.
pub fn parse_name_status(output: &str) -> Result<Vec<FileChange>> {
    let mut fields = output.split('\0').filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(code) = fields.next() {
        let bad = || Error::Parse(code.to_owned());
        let change = Change::from_code(code.as_bytes()[0]).ok_or_else(bad)?;
        let first = fields.next().ok_or_else(bad)?.to_owned();
        let (path, from) = if matches!(change, Change::Renamed | Change::Copied) {
            (fields.next().ok_or_else(bad)?.to_owned(), Some(first))
        } else {
            (first, None)
        };
        files.push(FileChange { path, from, change });
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_kind_of_record() {
        let output = [
            "# branch.oid 1234",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 A. N... 000000 100644 100644 aaa bbb with space.txt",
            "2 R. N... 100644 100644 100644 aaa bbb R100 new name.rs",
            "old name.rs",
            "u UU N... 100644 100644 100644 100644 a b c conflict.rs",
            "? notes.md",
            "! target",
            "",
        ]
        .join("\0");
        let status = Status::parse(&output).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        let summary: Vec<_> =
            status.entries.iter().map(|e| (e.path.as_str(), e.from.as_deref(), e.staged, e.unstaged)).collect();
        assert_eq!(
            summary,
            [
                ("src/main.rs", None, None, Some(Change::Modified)),
                ("with space.txt", None, Some(Change::Added), None),
                ("new name.rs", Some("old name.rs"), Some(Change::Renamed), None),
                ("conflict.rs", None, None, Some(Change::Conflicted)),
                ("notes.md", None, None, Some(Change::Untracked)),
            ]
        );
    }

    #[test]
    fn detached_head_has_no_branch() {
        let status = Status::parse("# branch.head (detached)\0").unwrap();
        assert_eq!(status.branch, None);
        assert!(status.is_clean());
    }

    #[test]
    fn rejects_what_it_does_not_know() {
        assert!(Status::parse("x something\0").is_err());
        assert!(Status::parse("1 .M\0").is_err());
        assert!(Status::parse("# branch.ab +x -1\0").is_err());
        assert!(parse_name_status("Z\0path\0").is_err());
    }

    #[test]
    fn parses_name_status() {
        let files = parse_name_status("M\0a.rs\0R087\0old.rs\0new.rs\0D\0gone.rs\0").unwrap();
        assert_eq!(
            files,
            [
                FileChange { path: "a.rs".into(), from: None, change: Change::Modified },
                FileChange { path: "new.rs".into(), from: Some("old.rs".into()), change: Change::Renamed },
                FileChange { path: "gone.rs".into(), from: None, change: Change::Deleted },
            ]
        );
    }
}
