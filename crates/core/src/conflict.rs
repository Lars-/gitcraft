//! A file git could not merge: the text both sides agree on and the conflicts git marked in it,
//! put back together with a side picked for each conflict.

use std::ops::Range;

use crate::git::Git;
use crate::{Error, Result};

/// What to keep of a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Our lines.
    Ours,
    /// Their lines.
    Theirs,
    /// Our lines, then theirs.
    Both,
}

impl Pick {
    /// Which of the sides of a [`Part::Conflict`] it keeps.
    pub fn sides(self) -> Range<usize> {
        match self {
            Self::Ours => 0..1,
            Self::Theirs => 1..2,
            Self::Both => 0..2,
        }
    }
}

/// A part of a conflicted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Text both sides agree on.
    Same(String),
    /// Lines each side changed its own way: ours, then theirs.
    Conflict([String; 2]),
}

/// A conflicted file, split at the markers git wrote into it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conflicts {
    /// What git calls our side and theirs, like `HEAD` and the branch coming in.
    pub names: [String; 2],
    /// The file, in order.
    pub parts: Vec<Part>,
}

/// Where a line of a conflicted file belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At {
    Same,
    Ours,
    /// The common ancestor, which `merge.conflictStyle=diff3` shows. It is left out.
    Base,
    Theirs,
}

impl Conflicts {
    /// Reads the conflicts in `path`, a file in the working tree.
    pub fn read(git: &Git, path: &str) -> Result<Self> {
        let bytes =
            std::fs::read(git.dir().join(path)).map_err(|source| Error::File { path: path.to_owned(), source })?;
        let text = String::from_utf8(bytes).ok();
        text.as_deref().and_then(Self::parse).ok_or_else(|| Error::NoConflicts(path.to_owned()))
    }

    /// Splits `text` at its conflict markers; `None` when it has none, or they do not add up.
    pub fn parse(text: &str) -> Option<Self> {
        let mut file = Self::default();
        let mut at = At::Same;
        for line in text.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            // Seven marker characters, then nothing or a space and the name of the side.
            let marker = |marker: &str| {
                let name = bare.strip_prefix(marker)?;
                (name.is_empty() || name.starts_with(' ')).then(|| name.trim().to_owned())
            };
            if at == At::Same
                && let Some(name) = marker("<<<<<<<")
            {
                file.names[0] = name;
                file.parts.push(Part::Conflict(Default::default()));
                at = At::Ours;
            } else if at == At::Ours && marker("|||||||").is_some() {
                at = At::Base;
            } else if matches!(at, At::Ours | At::Base) && bare == "=======" {
                at = At::Theirs;
            } else if at == At::Theirs
                && let Some(name) = marker(">>>>>>>")
            {
                file.names[1] = name;
                at = At::Same;
            } else {
                match (at, file.parts.last_mut()) {
                    (At::Same, Some(Part::Same(same))) => same.push_str(line),
                    (At::Same, _) => file.parts.push(Part::Same(line.to_owned())),
                    (At::Ours, Some(Part::Conflict(sides))) => sides[0].push_str(line),
                    (At::Theirs, Some(Part::Conflict(sides))) => sides[1].push_str(line),
                    _ => {}
                }
            }
        }
        (at == At::Same && file.count() > 0).then_some(file)
    }

    /// How many conflicts there are.
    pub fn count(&self) -> usize {
        self.parts.iter().filter(|part| matches!(part, Part::Conflict(_))).count()
    }

    /// The file with what `picks` keeps of each conflict, in order; `None` while a conflict has
    /// no pick.
    pub fn resolve(&self, picks: &[Option<Pick>]) -> Option<String> {
        let mut picks = picks.iter();
        let mut text = String::new();
        for part in &self.parts {
            match part {
                Part::Same(same) => text.push_str(same),
                Part::Conflict(sides) => sides[(*picks.next()?)?.sides()].iter().for_each(|side| text.push_str(side)),
            }
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MERGE: &str = "one\n<<<<<<< HEAD\nours\n=======\ntheirs\nmore\n>>>>>>> topic\ntwo\n";

    fn conflict(ours: &str, theirs: &str) -> Part {
        Part::Conflict([ours.into(), theirs.into()])
    }

    #[test]
    fn splits_a_file_at_its_conflict_markers() {
        let file = Conflicts::parse(MERGE).unwrap();
        assert_eq!(file.names, ["HEAD".to_owned(), "topic".to_owned()]);
        assert_eq!(
            file.parts,
            [Part::Same("one\n".into()), conflict("ours\n", "theirs\nmore\n"), Part::Same("two\n".into())]
        );
        assert_eq!(file.count(), 1);
    }

    #[test]
    fn leaves_out_the_common_ancestor() {
        let diff3 = MERGE.replace("=======", "||||||| base\nancestor\n=======");
        assert_eq!(Conflicts::parse(&diff3), Conflicts::parse(MERGE));
    }

    #[test]
    fn keeps_line_endings_and_a_missing_last_newline() {
        let file = Conflicts::parse("a\r\n<<<<<<< HEAD\r\nb\r\n=======\r\n>>>>>>> x\r\nend").unwrap();
        assert_eq!(file.parts, [Part::Same("a\r\n".into()), conflict("b\r\n", ""), Part::Same("end".into())]);
        assert_eq!(file.resolve(&[Some(Pick::Ours)]).unwrap(), "a\r\nb\r\nend");
        assert_eq!(file.resolve(&[Some(Pick::Theirs)]).unwrap(), "a\r\nend");
    }

    #[test]
    fn markers_count_only_where_they_belong() {
        assert_eq!(Conflicts::parse("no conflicts\n=======\n"), None);
        assert_eq!(Conflicts::parse("<<<<<<< HEAD\nnever ends\n=======\n"), None);
        assert_eq!(Conflicts::parse("<<<<<<<< eight is not seven\n"), None);
        let file = Conflicts::parse("=======\n<<<<<<<\n>>>>>>> x\n=======\n>>>>>>>\n").unwrap();
        assert_eq!(file.parts, [Part::Same("=======\n".into()), conflict(">>>>>>> x\n", "")]);
    }

    #[test]
    fn resolves_once_every_conflict_has_a_pick() {
        let twice = format!("{MERGE}{MERGE}");
        let file = Conflicts::parse(&twice).unwrap();
        assert_eq!(file.count(), 2);
        assert_eq!(file.resolve(&[Some(Pick::Ours), None]), None);
        assert_eq!(file.resolve(&[Some(Pick::Ours)]), None);
        assert_eq!(
            file.resolve(&[Some(Pick::Ours), Some(Pick::Both)]).unwrap(),
            "one\nours\ntwo\none\nours\ntheirs\nmore\ntwo\n"
        );
    }
}
