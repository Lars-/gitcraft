//! Colours, the look of the app, and how times read.

use eframe::egui::{self, Color32};
use gitcraft_core::date;
use gitcraft_core::status::Change;

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
/// The background of a line picked for staging.
pub const PICKED: Color32 = Color32::from_rgb(0x2b, 0x4a, 0x7a);
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
        Change::Conflicted => ("C", LANES[9]),
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

/// How long before `now` (seconds since 1970) `time` was, or its date when that is over a week.
pub fn when(time: date::Time, now: i64) -> String {
    let ago = now - time.seconds;
    let plural = |n: i64, unit: &str| format!("{} ago", count(n.unsigned_abs().try_into().unwrap_or(usize::MAX), unit));
    match ago {
        ..60 => "just now".to_owned(),
        60..3_600 => plural(ago / 60, "minute"),
        3_600..86_400 => plural(ago / 3_600, "hour"),
        86_400..604_800 => plural(ago / 86_400, "day"),
        _ => time.format_or_unix(date::time::format::SHORT),
    }
}

/// A number of things, like "1 file" or "3 files".
pub fn count(n: usize, thing: &str) -> String {
    format!("{n} {thing}{}", if n == 1 { "" } else { "s" })
}

/// Seconds since 1970, now.
pub fn now() -> i64 {
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
        assert_eq!(when(at(now - 5), now), "just now");
        assert_eq!(when(at(now - 60), now), "1 minute ago");
        assert_eq!(when(at(now - 7_200), now), "2 hours ago");
        assert_eq!(when(at(now - 3 * 86_400), now), "3 days ago");
        assert_eq!(when(at(1_700_000_000), now), "2023-11-14");
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
