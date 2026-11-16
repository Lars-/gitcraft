//! Unified diffs from `git diff`, the words that changed within lines, and patches built from
//! parts of them for staging.

use std::ops::Range;

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
    /// The line in the same hunk that this one replaces or is replaced by: in a block of changes,
    /// the first removed line pairs with the first added line, the second with the second.
    pub pair: Option<usize>,
}

/// A block of changes with its context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` line.
    pub header: String,
    /// The lines.
    pub lines: Vec<Line>,
}

impl Hunk {
    /// The removed and the added text of line `l` and the line it pairs with, for [`words`].
    pub fn pair(&self, l: usize) -> Option<[&str; 2]> {
        let line = &self.lines[l];
        let pair = &self.lines[line.pair?];
        let [old, new] = if line.kind == LineKind::Removed { [line, pair] } else { [pair, line] };
        Some([&old.text, &new.text])
    }
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
        /// Where the commit renamed it from, so that the diff shows the rename and not a new file.
        from: Option<String>,
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
        Source::Commit { commit, path, from } => git.run(
            &[
                &["show", "--format=", "--diff-merges=first-parent"][..],
                &OPTIONS,
                &[commit, "--", path],
                from.as_deref().as_slice(),
            ]
            .concat(),
        )?,
    };
    Ok(FileDiff::parse(&output).into_iter().next().unwrap_or_default())
}

impl FileDiff {
    /// Parses the output of `git diff`, one entry per file.
    pub fn parse(output: &str) -> Vec<Self> {
        let mut files: Vec<Self> = Vec::new();
        let (mut old, mut new) = (0, 0);
        // The removed lines of the block of changes so far, and how many lines it added.
        let (mut removed, mut added) = (Vec::new(), 0);
        for text in output.split_terminator('\n') {
            let in_hunk = files.last().is_some_and(|f| !f.hunks.is_empty());
            let marker = text.chars().next().unwrap_or(' ');
            let Some(file) = files.last_mut().filter(|_| !text.starts_with("diff --git ")) else {
                files.push(Self { header: vec![text.to_owned()], ..Self::default() });
                continue;
            };
            if let Some((o, n)) = text.strip_prefix("@@ ").and_then(hunk_start) {
                (old, new, added) = (o, n, 0);
                removed.clear();
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
                let lines = &mut file.hunks.last_mut().expect("in a hunk").lines;
                let pair = match kind {
                    LineKind::Removed => {
                        removed.push(lines.len());
                        None
                    }
                    LineKind::Added => {
                        added += 1;
                        removed.get(added - 1).copied()
                    }
                    LineKind::Context => {
                        removed.clear();
                        added = 0;
                        None
                    }
                    LineKind::NoNewline => None,
                };
                if let Some(p) = pair {
                    lines[p].pair = Some(lines.len());
                }
                lines.push(Line {
                    kind,
                    text: text.get(1..).unwrap_or_default().to_owned(),
                    old: numbers.0,
                    new: numbers.1,
                    pair,
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

/// Lines with more words than this, after what they start and end with in common, are only
/// compared on that start and end: the table that finds the words in common grows with the
/// product of the lengths.
const WORDS: usize = 256;

/// The parts of `old` and `new` that the other does not have, as byte ranges in each: the words,
/// runs of white space and other characters left over once the longest run of them both have in
/// order is taken out. None when less than half of the two (white space aside) is the same, as
/// for a line rewritten as a whole.
///
/// This is for a removed line and the added line it pairs with ([`Line::pair`]), on screen: its
/// time grows with the product of the lengths of the two, up to a few hundred words each.
pub fn words(old: &str, new: &str) -> [Vec<Range<usize>>; 2] {
    let (olds, news) = (tokens(old), tokens(new));
    let same = |i: usize, j: usize| old[olds[i].clone()] == new[news[j].clone()];
    let prefix = (0..olds.len().min(news.len())).take_while(|&i| same(i, i)).count();
    let suffix =
        (0..olds.len().min(news.len()) - prefix).take_while(|&k| same(olds.len() - 1 - k, news.len() - 1 - k)).count();
    let (n, m) = (olds.len() - prefix - suffix, news.len() - prefix - suffix);
    let mut kept = [vec![true; olds.len()], vec![true; news.len()]];
    kept[0][prefix..prefix + n].fill(false);
    kept[1][prefix..prefix + m].fill(false);
    if n.min(m) > 0 && n.max(m) <= WORDS {
        // `table[i * (m + 1) + j]` is how many tokens the middles have in common from `i` and `j` on.
        let at = |i: usize, j: usize| i * (m + 1) + j;
        let mut table = vec![0u16; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                table[at(i, j)] = if same(prefix + i, prefix + j) {
                    table[at(i + 1, j + 1)] + 1
                } else {
                    table[at(i + 1, j)].max(table[at(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if same(prefix + i, prefix + j) {
                (kept[0][prefix + i], kept[1][prefix + j]) = (true, true);
                (i, j) = (i + 1, j + 1);
            } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    let weight = |text: &str, tokens: &[Range<usize>], only_kept: Option<&[bool]>| -> usize {
        let counts = |i: usize, token: &Range<usize>| {
            only_kept.is_none_or(|kept| kept[i]) && !text[token.clone()].trim().is_empty()
        };
        tokens.iter().enumerate().filter(|&(i, token)| counts(i, token)).map(|(_, token)| token.len()).sum()
    };
    if 4 * weight(old, &olds, Some(&kept[0])) < weight(old, &olds, None) + weight(new, &news, None) {
        return [Vec::new(), Vec::new()];
    }
    [spans(&olds, &kept[0]), spans(&news, &kept[1])]
}

/// A line split into words, runs of white space and single other characters, as byte ranges.
fn tokens(text: &str) -> Vec<Range<usize>> {
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            1
        } else if c.is_whitespace() {
            2
        } else {
            0
        }
    };
    let mut tokens: Vec<Range<usize>> = Vec::new();
    let mut last = 0;
    for (i, c) in text.char_indices() {
        let kind = class(c);
        match tokens.last_mut() {
            Some(token) if kind != 0 && kind == last => token.end = i + c.len_utf8(),
            _ => tokens.push(i..i + c.len_utf8()),
        }
        last = kind;
    }
    tokens
}

/// The tokens that are not kept, with the ones next to each other joined.
fn spans(tokens: &[Range<usize>], kept: &[bool]) -> Vec<Range<usize>> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    for (token, _) in tokens.iter().zip(kept).filter(|&(_, &kept)| !kept) {
        match spans.last_mut() {
            Some(span) if span.end == token.start => span.end = token.end,
            _ => spans.push(token.clone()),
        }
    }
    spans
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
    fn lines_that_replace_each_other_pair_up() {
        let pairs = |diff: &str| -> Vec<Option<usize>> {
            FileDiff::parse(diff)[0].hunks[0].lines.iter().map(|line| line.pair).collect()
        };
        // "-two" and "+TWO" replace each other, "+extra" and "-four" stand alone.
        assert_eq!(pairs(DIFF), [None, Some(2), Some(1), None, None, None, None]);
        let more =
            "diff --git a/f b/f\n@@ -1,3 +1,4 @@\n-a\n-b\n+A\n+B\n+C\n x\n-y\n\\ No newline at end of file\n+Y\n";
        assert_eq!(pairs(more), [Some(2), Some(3), Some(0), Some(1), None, None, Some(8), None, Some(6)]);
    }

    const NONE: [Vec<String>; 2] = [Vec::new(), Vec::new()];

    fn words_of(old: &str, new: &str) -> [Vec<String>; 2] {
        let [a, b] = words(old, new);
        [a.into_iter().map(|r| old[r].to_owned()).collect(), b.into_iter().map(|r| new[r].to_owned()).collect()]
    }

    #[test]
    fn words_that_changed_within_a_line() {
        assert_eq!(words_of("let total = price * count;", "let total = price * amount;"), [["count"], ["amount"]]);
        // Words next to each other join; what only one side has stands alone.
        assert_eq!(words_of("call(a, b)", "call(a, b, c)"), [vec![], vec![", c".to_owned()]]);
        assert_eq!(words_of("if x == 1 { go() }", "if y == 1 { stop() }"), [["x", "go"], ["y", "stop"]]);
        // White space counts as a change, not towards being alike.
        assert_eq!(words_of("a  b", "\ta b"), [vec!["  ".to_owned()], vec!["\t".to_owned(), " ".to_owned()]]);
        assert_eq!(words_of("caf\u{e9} au lait", "caf\u{e9} au chocolat"), [["lait"], ["chocolat"]]);
    }

    #[test]
    fn lines_with_little_in_common_have_no_words() {
        assert_eq!(words_of("two", "TWO"), NONE);
        assert_eq!(words_of("    }", "    return x;"), NONE);
        assert_eq!(words_of("", "new"), NONE);
    }

    #[test]
    fn long_lines_compare_on_what_they_start_and_end_with() {
        let long = "word ".repeat(WORDS);
        let [old, new] = words(&format!("start {long}end"), &format!("start {long}stop"));
        assert_eq!((old.len(), new.len()), (1, 1));
        // Changes at both ends of a long middle leave too little in common to tell.
        assert_eq!(words_of(&format!("a {long}b"), &format!("c {long}d")), NONE);
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
        assert_eq!(Source::Commit { commit: "abc".into(), path: "a/b.rs".into(), from: None }.path(), "a/b.rs");
        assert_eq!(Source::Untracked("n.txt".into()).path(), "n.txt");
    }

    #[test]
    fn hunk_start_reads_single_line_ranges() {
        assert_eq!(hunk_start("-7 +9,0 @@"), Some((7, 9)));
        assert_eq!(hunk_start("garbage"), None);
    }
}
