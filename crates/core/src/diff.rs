//! Unified diffs from `git diff`, and patches built from parts of them for staging.

use crate::Result;
use crate::git::Git;

/// What a diff line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Unchanged.
    Context,
    /// Only in the new version.
    Added,
    /// Only in the old version.
    Removed,
    /// The line before it has no newline at the end.
    NoNewline,
}

/// One line of a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// What the line is.
    pub kind: LineKind,
    /// The text, without the leading marker.
    pub text: String,
    /// The line number in the old version.
    pub old: Option<u32>,
    /// The line number in the new version.
    pub new: Option<u32>,
}

/// A block of changes with its context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` line.
    pub header: String,
    /// The lines.
    pub lines: Vec<Line>,
}

/// The diff of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// The lines before the first hunk: `diff --git`, modes, `---` and `+++`.
    pub header: Vec<String>,
    /// The changes.
    pub hunks: Vec<Hunk>,
    /// Whether git considers the file binary.
    pub binary: bool,
}

/// Which diff of a file to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The working tree against the index.
    Unstaged(String),
    /// The index against `HEAD`.
    Staged(String),
    /// A file git does not track yet, as if all of it were new.
    Untracked(String),
    /// What a commit changed in a file, against its first parent.
    Commit {
        /// The commit.
        commit: String,
        /// The file.
        path: String,
    },
}

impl Source {
    /// The file the diff is about.
    pub fn path(&self) -> &str {
        match self {
            Self::Unstaged(path) | Self::Staged(path) | Self::Untracked(path) | Self::Commit { path, .. } => path,
        }
    }
}

const OPTIONS: [&str; 3] = ["--no-ext-diff", "--no-color", "-M"];

/// Reads one diff of one file.
pub fn read(git: &Git, source: &Source) -> Result<FileDiff> {
    let output = match source {
        Source::Unstaged(path) => git.run(&[&["diff"][..], &OPTIONS, &["--", path]].concat())?,
        Source::Staged(path) => git.run(&[&["diff", "--cached"][..], &OPTIONS, &["--", path]].concat())?,
        // `--no-index` exits with 1 when the files differ, which they always do here.
        Source::Untracked(path) => {
            git.run_with(&[&["diff", "--no-index"][..], &OPTIONS, &["--", "/dev/null", path]].concat(), None, &[0, 1])?
        }
        Source::Commit { commit, path } => git.run(
            &[&["show", "--format=", "--diff-merges=first-parent"][..], &OPTIONS, &[commit, "--", path]].concat(),
        )?,
    };
    Ok(FileDiff::parse(&output).into_iter().next().unwrap_or_default())
}

impl FileDiff {
    /// Parses the output of `git diff`, one entry per file.
    pub fn parse(output: &str) -> Vec<Self> {
        let mut files: Vec<Self> = Vec::new();
        let (mut old, mut new) = (0, 0);
        for text in output.split_terminator('\n') {
            let in_hunk = files.last().is_some_and(|f| !f.hunks.is_empty());
            let marker = text.chars().next().unwrap_or(' ');
            let Some(file) = files.last_mut().filter(|_| !text.starts_with("diff --git ")) else {
                files.push(Self { header: vec![text.to_owned()], ..Self::default() });
                continue;
            };
            if let Some((o, n)) = text.strip_prefix("@@ ").and_then(hunk_start) {
                (old, new) = (o, n);
                file.hunks.push(Hunk { header: text.to_owned(), lines: Vec::new() });
            } else if in_hunk && matches!(marker, ' ' | '+' | '-' | '\\') {
                let kind = match marker {
                    ' ' => LineKind::Context,
                    '+' => LineKind::Added,
                    '-' => LineKind::Removed,
                    _ => LineKind::NoNewline,
                };
                let numbers = match kind {
                    LineKind::Context => (Some(old), Some(new)),
                    LineKind::Removed => (Some(old), None),
                    LineKind::Added => (None, Some(new)),
                    LineKind::NoNewline => (None, None),
                };
                old += u32::from(numbers.0.is_some());
                new += u32::from(numbers.1.is_some());
                let hunk = file.hunks.last_mut().expect("in a hunk");
                hunk.lines.push(Line {
                    kind,
                    text: text.get(1..).unwrap_or_default().to_owned(),
                    old: numbers.0,
                    new: numbers.1,
                });
            } else {
                file.binary |= text.starts_with("Binary files ") || text == "GIT binary patch";
                file.header.push(text.to_owned());
            }
        }
        files
    }

    /// A patch with only the selected lines, for [`crate::git::Op::ApplyToIndex`].
    ///
    /// `selected` gets the hunk and line index. Hunks without a selected change are left out.
    /// Lines that are not selected stay as they are in the index: when staging, an unselected
    /// removal becomes context and an unselected addition is left out. With `reverse`, for taking
    /// lines out of the index, it is the other way around.
    pub fn patch(&self, selected: impl Fn(usize, usize) -> bool, reverse: bool) -> String {
        let mut patch = self.header.join("\n");
        patch.push('\n');
        for (h, hunk) in self.hunks.iter().enumerate() {
            let changed = |(i, line): (usize, &Line)| line.kind != LineKind::Context && selected(h, i);
            if !hunk.lines.iter().enumerate().any(changed) {
                continue;
            }
            patch.push_str(&hunk.header);
            patch.push('\n');
            let mut kept = true;
            for (i, line) in hunk.lines.iter().enumerate() {
                let marker = match (line.kind, selected(h, i), reverse) {
                    (LineKind::Context, ..) | (LineKind::Removed, false, false) | (LineKind::Added, false, true) => {
                        Some(' ')
                    }
                    (LineKind::Added, true, _) => Some('+'),
                    (LineKind::Removed, true, _) => Some('-'),
                    (LineKind::NoNewline, ..) => kept.then_some('\\'),
                    _ => None,
                };
                kept = marker.is_some();
                if let Some(marker) = marker {
                    patch.push(marker);
                    patch.push_str(&line.text);
                    patch.push('\n');
                }
            }
        }
        patch
    }
}

/// The first old and new line numbers from the rest of a `@@ -a,b +c,d @@` line.
fn hunk_start(header: &str) -> Option<(u32, u32)> {
    let mut ranges = header.split(' ');
    let start = |range: Option<&str>, sign| range?.strip_prefix(sign)?.split(',').next()?.parse().ok();
    Some((start(ranges.next(), '-')?, start(ranges.next(), '+')?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/f.txt b/f.txt
index 1111111..2222222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,4 +1,4 @@ intro
 one
-two
+TWO
+extra
 three
-four
\\ No newline at end of file
diff --git a/img.png b/img.png
index 3333333..4444444 100644
Binary files a/img.png and b/img.png differ
";

    #[test]
    fn parses_files_hunks_and_line_numbers() {
        let files = FileDiff::parse(DIFF);
        assert_eq!(files.len(), 2);
        let file = &files[0];
        assert_eq!(file.header.len(), 4);
        assert!(!file.binary && files[1].binary);
        let lines = &file.hunks[0].lines;
        let summary: Vec<_> = lines.iter().map(|l| (l.kind, l.text.as_str(), l.old, l.new)).collect();
        assert_eq!(
            summary,
            [
                (LineKind::Context, "one", Some(1), Some(1)),
                (LineKind::Removed, "two", Some(2), None),
                (LineKind::Added, "TWO", None, Some(2)),
                (LineKind::Added, "extra", None, Some(3)),
                (LineKind::Context, "three", Some(3), Some(4)),
                (LineKind::Removed, "four", Some(4), None),
                (LineKind::NoNewline, " No newline at end of file", None, None),
            ]
        );
    }

    #[test]
    fn whole_hunk_patch_is_the_hunk() {
        let file = &FileDiff::parse(DIFF)[0];
        let expected = DIFF.split("diff --git a/img").next().unwrap();
        assert_eq!(file.patch(|_, _| true, false), expected);
    }

    #[test]
    fn partial_patch_keeps_unselected_lines_as_they_are() {
        let file = &FileDiff::parse(DIFF)[0];
        let body = |patch: String| patch.lines().skip(5).map(str::to_owned).collect::<Vec<_>>();
        // Stage only "+TWO": "-two" becomes context, "+extra" and "-four" stay out.
        assert_eq!(
            body(file.patch(|_, i| i == 2, false)),
            [" one", " two", "+TWO", " three", " four", "\\ No newline at end of file"]
        );
        // Unstage only "+extra": the other additions are in the index already, the removals are not.
        assert_eq!(body(file.patch(|_, i| i == 3, true)), [" one", " TWO", "+extra", " three"]);
    }

    #[test]
    fn hunks_without_a_selected_change_are_left_out() {
        let two_hunks = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+A\n@@ -9 +9 @@\n-b\n+B\n";
        let file = &FileDiff::parse(two_hunks)[0];
        assert_eq!(file.patch(|h, _| h == 1, false), "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -9 +9 @@\n-b\n+B\n");
        assert_eq!(file.patch(|_, i| i == 5, false), "diff --git a/f b/f\n--- a/f\n+++ b/f\n");
    }

    #[test]
    fn source_knows_its_path() {
        assert_eq!(Source::Commit { commit: "abc".into(), path: "a/b.rs".into() }.path(), "a/b.rs");
        assert_eq!(Source::Untracked("n.txt".into()).path(), "n.txt");
    }

    #[test]
    fn hunk_start_reads_single_line_ranges() {
        assert_eq!(hunk_start("-7 +9,0 @@"), Some((7, 9)));
        assert_eq!(hunk_start("garbage"), None);
    }
}
