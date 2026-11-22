//! Colours, the look of the app, and how times read.

use eframe::egui::{self, Color32};
use gitcraft_core::date;
use gitcraft_core::status::Change;

use crate::settings;

/// One colour per graph lane, repeating.
const LANES: [Color32; 10] = [
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
];

/// The background of an added line.
pub const ADDED: Color32 = Color32::from_rgb(0x1d, 0x3b, 0x2c);
/// The background of a removed line.
pub const REMOVED: Color32 = Color32::from_rgb(0x45, 0x22, 0x27);
/// The background of the words that changed in an added line.
pub const ADDED_WORD: Color32 = Color32::from_rgb(0x2c, 0x63, 0x45);
/// The background of the words that changed in a removed line.
pub const REMOVED_WORD: Color32 = Color32::from_rgb(0x74, 0x30, 0x39);
/// The background of a conflict without a pick.
pub const CONFLICT: Color32 = Color32::from_rgb(0x4a, 0x38, 0x1c);
/// The background of a line picked for staging.
pub const PICKED: Color32 = Color32::from_rgb(0x2b, 0x4a, 0x7a);
/// The colours of code: one of syntect's own themes, made for a dark background.
pub const CODE: &str = "base16-ocean.dark";
/// Text for errors.
pub const ERROR: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);

/// The colour of a lane.
pub fn lane(i: usize) -> Color32 {
    LANES[i % LANES.len()]
}

/// The letter and colour that mark a kind of change.
pub fn change(change: Change) -> (&'static str, Color32) {
    match change {
        Change::Added => ("A", LANES[4]),
        Change::Modified => ("M", LANES[3]),
        Change::Deleted => ("D", LANES[5]),
        Change::Renamed | Change::Copied => ("R", LANES[0]),
        Change::TypeChanged => ("T", LANES[1]),
        Change::Untracked => ("U", LANES[7]),
        Change::Conflicted { .. } => ("C", LANES[9]),
    }
}

/// Sets up the dark look.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals.panel_fill = Color32::from_rgb(0x1f, 0x22, 0x28);
        style.visuals.extreme_bg_color = Color32::from_rgb(0x17, 0x19, 0x1e);
        style.visuals.selection.bg_fill = Color32::from_rgb(0x2b, 0x4a, 0x7a);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
    });
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
        assert_eq!(lane(1), lane(11));
        assert_ne!(lane(1), lane(2));
    }
}
