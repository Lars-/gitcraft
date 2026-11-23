//! Colours for code, from the grammar of its language: syntect's, for some fifty languages.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

use eframe::egui::{Color32, Theme};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::theme;

/// The grammars, read the first time code is coloured.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// syntect's own colours for code, for a dark and for a light background among them.
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Code larger than this keeps the colour of plain text: colouring takes some 50 µs a line.
const LARGEST: usize = 512 << 10;

/// A line of code, with its colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The text, without the line ending.
    pub text: String,
    /// Where in the text each colour starts; `None` is the colour of plain text.
    pub colours: Vec<(usize, Option<Color32>)>,
}

/// The lines of `code`, the whole of a file at `path`, with their colours in `theme`. A line takes
/// its colour from the lines before it, as one in a comment over several lines does, so this goes
/// through the file from the start. Nothing when the language is not known, or the code is not
/// text or too large.
pub fn colour(path: &str, code: &[u8], theme: Theme) -> Vec<Line> {
    let code = std::str::from_utf8(code).ok().filter(|code| code.len() <= LARGEST);
    let Some((code, syntax)) = code.and_then(|code| Some((code, syntax(path, code)?))) else { return Vec::new() };
    let theme = &THEMES.themes[theme::colours(theme).code];
    let plain = theme.settings.foreground;
    let mut highlighter = HighlightLines::new(syntax, theme);
    LinesWithEndings::from(code)
        .map_while(|line| {
            let pieces = highlighter.highlight_line(line, &SYNTAXES).ok()?;
            let text = line.strip_suffix('\n').unwrap_or(line).trim_end_matches('\r');
            let mut colours = Vec::new();
            let mut at = 0;
            for (style, piece) in pieces {
                let colour = Some(style.foreground).filter(|&colour| Some(colour) != plain);
                let colour = colour.map(|colour| Color32::from_rgb(colour.r, colour.g, colour.b));
                if at < text.len() && colours.last().is_none_or(|&(_, last)| last != colour) {
                    colours.push((at, colour));
                }
                at += piece.len();
            }
            Some(Line { text: text.to_owned(), colours })
        })
        .collect()
}

/// The grammar for a file at `path` with `code` in it: by its name, its extension, or a first line
/// like `#!/bin/sh`.
fn syntax(path: &str, code: &str) -> Option<&'static SyntaxReference> {
    let path = Path::new(path);
    let by = |part: Option<&OsStr>| SYNTAXES.find_syntax_by_extension(part?.to_str()?);
    by(path.file_name())
        .or_else(|| by(path.extension()))
        .or_else(|| SYNTAXES.find_syntax_by_first_line(code.lines().next()?))
        .filter(|syntax| syntax.name != SYNTAXES.find_syntax_plain_text().name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The colour `line` has at `text` in it.
    fn at(line: &Line, text: &str) -> Option<Color32> {
        let start = line.text.find(text).expect("the text is in the line");
        line.colours.iter().rev().find(|&&(at, _)| at <= start).and_then(|&(_, colour)| colour)
    }

    #[test]
    fn code_takes_the_colours_of_its_language() {
        let lines = colour("src/main.rs", b"fn main() {\n    let greeting = \"hello\"; // a comment\n}\n", Theme::Dark);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].text, "    let greeting = \"hello\"; // a comment");
        let [keyword, string, comment] = ["let", "hello", "a comment"].map(|text| at(&lines[1], text));
        assert!(keyword.is_some() && string.is_some() && comment.is_some());
        assert!(keyword != string && string != comment && comment != keyword);
        assert_eq!(at(&lines[1], "greeting"), None, "a name has the colour of plain text");
        assert_eq!(at(&lines[0], "fn"), keyword);
        let light = colour("src/main.rs", b"fn main() {}\n", Theme::Light);
        assert!(at(&light[0], "fn").is_some_and(|light| Some(light) != keyword), "other colours on light");
    }

    #[test]
    fn a_line_takes_its_colour_from_the_lines_before_it() {
        let lines = colour("a.c", b"/* a comment\nover two lines */\nint x;\r\n", Theme::Dark);
        assert_eq!(at(&lines[1], "over"), at(&lines[0], "a comment"));
        assert_ne!(at(&lines[2], "int"), at(&lines[0], "a comment"));
        assert_eq!(lines[2].text, "int x;");
    }

    #[test]
    fn a_file_without_an_extension_is_known_by_its_name_or_first_line() {
        assert_ne!(colour("Makefile", b"all:\n\tcc main.c\n", Theme::Dark), []);
        assert_ne!(colour("bin/run", b"#!/bin/sh\necho hello\n", Theme::Dark), []);
    }

    #[test]
    fn text_unknown_code_and_large_files_stay_plain() {
        assert_eq!(colour("notes.txt", b"fn main() {}\n", Theme::Dark), []);
        assert_eq!(colour("data.unknown", b"fn main() {}\n", Theme::Dark), []);
        assert_eq!(colour("main.rs", b"fn main() { \xff }\n", Theme::Dark), []);
        assert_eq!(colour("main.rs", "fn main() {}\n".repeat(50_000).as_bytes(), Theme::Dark), []);
    }
}
