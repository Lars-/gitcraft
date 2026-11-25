//! Colours, the look of the app, and how times read.

use eframe::egui::{self, Color32, Theme};
use gitcraft_core::date;
use gitcraft_core::graph::COLOURS;
use gitcraft_core::status::Change;

use crate::settings;

/// The colours the app adds to egui's own, for a dark or a light background.
#[derive(Debug)]
pub struct Colours {
    /// The colours the lines of the graph take turns in.
    lanes: [Color32; COLOURS as usize],
    /// The background of an added line.
    pub added: Color32,
    /// The background of a removed line.
    pub removed: Color32,
    /// The background of the words that changed in an added line.
    pub added_word: Color32,
    /// The background of the words that changed in a removed line.
    pub removed_word: Color32,
    /// The background of a conflict without a pick.
    pub conflict: Color32,
    /// The background of a tag in the graph.
    pub tag: Color32,
    /// The colours of code: one of syntect's own themes.
    pub code: &'static str,
}

/// The colours on a dark background.
pub const DARK: Colours = Colours {
    lanes: [
        Color32::from_rgb(0x38, 0xbd, 0xf8),
        Color32::from_rgb(0xa7, 0x8b, 0xfa),
        Color32::from_rgb(0xf4, 0x72, 0xb6),
        Color32::from_rgb(0xfb, 0xbf, 0x24),
        Color32::from_rgb(0x34, 0xd3, 0x99),
        Color32::from_rgb(0xf8, 0x71, 0x71),
        Color32::from_rgb(0x81, 0x8c, 0xf8),
        Color32::from_rgb(0xa3, 0xe6, 0x35),
        Color32::from_rgb(0x22, 0xd3, 0xee),
        Color32::from_rgb(0xfb, 0x92, 0x3c),
    ],
    added: Color32::from_rgb(0x1d, 0x3b, 0x2c),
    removed: Color32::from_rgb(0x45, 0x22, 0x27),
    added_word: Color32::from_rgb(0x2c, 0x63, 0x45),
    removed_word: Color32::from_rgb(0x74, 0x30, 0x39),
    conflict: Color32::from_rgb(0x4a, 0x38, 0x1c),
    tag: Color32::from_gray(70),
    code: "base16-ocean.dark",
};

/// The colours on a light background: the same, a shade darker for lines and text, and lighter
/// behind text.
pub const LIGHT: Colours = Colours {
    lanes: [
        Color32::from_rgb(0x02, 0x84, 0xc7),
        Color32::from_rgb(0x7c, 0x3a, 0xed),
        Color32::from_rgb(0xdb, 0x27, 0x77),
        Color32::from_rgb(0xd9, 0x77, 0x06),
        Color32::from_rgb(0x05, 0x96, 0x69),
        Color32::from_rgb(0xdc, 0x26, 0x26),
        Color32::from_rgb(0x4f, 0x46, 0xe5),
        Color32::from_rgb(0x65, 0xa3, 0x0d),
        Color32::from_rgb(0x08, 0x91, 0xb2),
        Color32::from_rgb(0xea, 0x58, 0x0c),
    ],
    added: Color32::from_rgb(0xdc, 0xfc, 0xe7),
    removed: Color32::from_rgb(0xfe, 0xe2, 0xe2),
    added_word: Color32::from_rgb(0x86, 0xef, 0xac),
    removed_word: Color32::from_rgb(0xfc, 0xa5, 0xa5),
    conflict: Color32::from_rgb(0xfe, 0xf3, 0xc7),
    tag: Color32::from_gray(215),
    code: "InspiredGitHub",
};

/// The colours for `theme`.
pub fn colours(theme: Theme) -> &'static Colours {
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

impl Colours {
    /// The colour of a line in the graph.
    pub fn lane(&self, i: u8) -> Color32 {
        self.lanes[usize::from(i) % self.lanes.len()]
    }

    /// The letter and colour that mark a kind of change.
    pub fn change(&self, change: Change) -> (&'static str, Color32) {
        let (letter, lane) = match change {
            Change::Added => ("A", 4),
            Change::Modified => ("M", 3),
            Change::Deleted => ("D", 5),
            Change::Renamed | Change::Copied => ("R", 0),
            Change::TypeChanged => ("T", 1),
            Change::Untracked => ("U", 7),
            Change::Conflicted { .. } => ("C", 9),
        };
        (letter, self.lanes[lane])
    }
}

/// Sets up the look, dark and light. Which one shows is egui's theme preference, which eframe saves
/// between runs: dark, light, or the one the system uses.
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| style.spacing.button_padding = egui::vec2(8.0, 3.0));
    ctx.style_mut_of(Theme::Dark, |style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = Color32::from_rgb(0x1f, 0x22, 0x28);
        visuals.extreme_bg_color = Color32::from_rgb(0x17, 0x19, 0x1e);
        visuals.selection.bg_fill = Color32::from_rgb(0x2b, 0x4a, 0x7a);
        visuals.error_fg_color = Color32::from_rgb(0xf8, 0x71, 0x71);
    });
    ctx.style_mut_of(Theme::Light, |style| style.visuals.error_fg_color = Color32::from_rgb(0xdc, 0x26, 0x26));
}

/// How the dates of commits read: how long ago, or the date and the time.
#[derive(Debug, Clone, Copy)]
pub struct Dates {
    /// Seconds since 1970, now.
    now: i64,
    /// The date and the time, rather than how long ago.
    full: bool,
}

/// A date and a time, to the minute.
const FULL: date::time::CustomFormat = date::time::CustomFormat::new("%Y-%m-%d %H:%M");

impl Dates {
    /// As the settings have them, now.
    pub fn of(ctx: &egui::Context) -> Self {
        Self { now: now(), full: settings::FULL_DATES.on(ctx) }
    }

    /// When `time` was: how long before now, or its date when that is over a week; or else the date
    /// and the time.
    pub fn when(self, time: date::Time) -> String {
        let ago = self.now - time.seconds;
        let plural =
            |n: i64, unit: &str| format!("{} ago", count(n.unsigned_abs().try_into().unwrap_or(usize::MAX), unit));
        match ago {
            _ if self.full => time.format_or_unix(FULL),
            ..60 => "just now".to_owned(),
            60..3_600 => plural(ago / 60, "minute"),
            3_600..86_400 => plural(ago / 3_600, "hour"),
            86_400..604_800 => plural(ago / 86_400, "day"),
            _ => time.format_or_unix(date::time::format::SHORT),
        }
    }

    /// The room a date takes in a list, with a margin on either side.
    pub fn width(self) -> f32 {
        if self.full { 128.0 } else { 100.0 }
    }
}

/// A number of things, like "1 file" or "3 files".
pub fn count(n: usize, thing: &str) -> String {
    format!("{n} {thing}{}", if n == 1 { "" } else { "s" })
}

/// A word with a capital first letter, like "Rebase".
pub fn capital(word: &str) -> String {
    let (first, rest) = word.split_at(word.chars().next().map_or(0, char::len_utf8));
    format!("{}{rest}", first.to_uppercase())
}

/// Seconds since 1970, now.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_naturally() {
        let at = |seconds| date::Time { seconds, offset: 3_600 };
        let now = 2_000_000_000;
        let when = |seconds| Dates { now, full: false }.when(at(seconds));
        assert_eq!(when(now - 5), "just now");
        assert_eq!(when(now - 60), "1 minute ago");
        assert_eq!(when(now - 7_200), "2 hours ago");
        assert_eq!(when(now - 3 * 86_400), "3 days ago");
        assert_eq!(when(1_700_000_000), "2023-11-14");
        // In the time zone of the commit.
        assert_eq!(Dates { now, full: true }.when(at(now - 5)), "2033-05-18 04:33");
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(count(1, "file"), "1 file");
        assert_eq!(count(0, "file"), "0 files");
    }

    #[test]
    fn lanes_repeat_their_colours() {
        assert_eq!(DARK.lane(1), DARK.lane(11));
        assert_ne!(DARK.lane(1), DARK.lane(2));
        assert_ne!(DARK.lane(1), LIGHT.lane(1));
    }
}
