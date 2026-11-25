//! The commit graph: one row per commit, with its branches, lanes and message, and a search
//! above it. A commit dragged onto another moves to its place, in an interactive rebase. The
//! borders between the names of the columns make them wider or narrower.

use eframe::egui::epaint::CubicBezierShape;
use eframe::egui::{
    self, Button, Color32, CursorIcon, FontId, Id, Key, KeyboardShortcut, Modifiers, Painter, Pos2, Rect, Response,
    RichText, Role, Sense, Shape, Stroke, TextEdit, Ui, Vec2, WidgetInfo,
};
use gitcraft_core::ObjectId;
use gitcraft_core::git::{Op, ResetMode};
use gitcraft_core::graph::{Half, Row};
use gitcraft_core::repo::{History, Label as Ref, RefKind};

use crate::app::{Action, Confirm, Prompt, Selection, Workspace};
use crate::theme;

/// The height of a row.
pub const ROW: f32 = 28.0;
const LANE: f32 = 16.0;
const LABELS: f32 = 170.0;
const AUTHOR: f32 = 140.0;
/// The height of the names of the columns.
const HEADER: f32 = 20.0;

/// A count as a distance in points.
pub fn points(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// Shows the search and the graph.
pub fn show(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    search(ui, ws, actions);
    let wip = usize::from(!ws.snapshot.status.is_clean());
    let total = ws.snapshot.history.ids.len() + wip;
    arrow_keys(ui, ws, actions);
    let columns = header(ui, ws.snapshot.history.graph.width());
    let mut area = egui::ScrollArea::vertical().auto_shrink(false);
    if let Some(row) = ws.scroll_to.take()
        && !ws.visible.contains(&row)
    {
        area = area.vertical_scroll_offset(points(row.saturating_sub(5)) * ROW);
    }
    let selected = ws.selected_row();
    let current = ws.snapshot.status.branch.clone();
    let dates = theme::Dates::of(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_rows(ui, ROW, total, |ui, range| {
        // egui draws a row or two more than fit; only the rows wholly on screen count as visible.
        let screen = ui.clip_rect();
        ws.visible = range.end..range.end;
        let history = &ws.snapshot.history;
        let Columns { labels: labels_width, lanes, author: author_width, date: date_width, .. } = columns;
        let first = range.start.saturating_sub(wip);
        let rows = history.graph.rows(first..range.end.saturating_sub(wip));
        let head_lane = history.head.and_then(|head| history.graph.rows(head..head + 1).pop()).map_or(0, |r| r.lane);
        for i in range {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click_and_drag());
            if screen.contains_rect(rect) {
                ws.visible = ws.visible.start.min(i)..i + 1;
            }
            let painter = ui.painter_at(rect);
            if selected == Some(i) {
                painter.rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
            } else if response.hovered() {
                painter.rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            let graph = Rect::from_min_size(rect.min + Vec2::X * labels_width, Vec2::new(lanes, ROW));
            let lane_x = |lane: usize| graph.left() + LANE * points(lane + 1);
            let text =
                Rect::from_x_y_ranges(graph.right() + 8.0..=rect.right() - author_width - date_width, rect.y_range());
            if i < wip {
                let node = Pos2::new(lane_x(head_lane), rect.center().y);
                if history.head == Some(0) {
                    dashed(&painter, node, Pos2::new(node.x, rect.bottom()));
                }
                painter.circle(node, 6.0, ui.visuals().panel_fill, Stroke::new(2.0, Color32::GRAY));
                let changes = ws.snapshot.status.entries.len();
                put(ui, text, egui::RichText::new(format!("// WIP  ({changes} changed)")).italics());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Uncommitted changes"));
                if response.clicked() {
                    actions.push(Action::Select(Selection::Wip));
                }
                continue;
            }
            let index = i - wip;
            let row = &rows[index - first];
            let id = history.ids[index];
            let labels = history.labels.get(&index).map_or(&[][..], Vec::as_slice);
            let head = history.head == Some(index);
            let merge = history.graph.parents(index).len() > 1;
            paint_row(&painter.with_clip_rect(graph), row, lane_x, merge, (head, head && wip == 1 && index == 0));
            let colour = theme::colours(ui.ctx().theme()).lane(row.colour);
            paint_labels(&painter, rect.with_max_x(rect.left() + labels_width), labels, colour, current.as_deref());
            match ws.commits.get(&ws.repo, id) {
                Ok(commit) => {
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, commit.summary()));
                    if response.dragged() {
                        let tooltip = egui::Tooltip::for_widget(&response).at_pointer();
                        tooltip.show(|ui| ui.label(format!("Move {}", commit.summary())));
                    }
                    let summary = RichText::new(commit.summary());
                    put(ui, text, if ws.search.misses(index) { summary.weak() } else { summary });
                    if author_width > 0.0 {
                        let author =
                            Rect::from_x_y_ranges(text.right() + 8.0..=text.right() + author_width, rect.y_range());
                        put(ui, author, egui::RichText::new(&commit.author).weak());
                    }
                    let date = Rect::from_x_y_ranges(rect.right() - date_width..=rect.right() - 8.0, rect.y_range());
                    put(ui, date, egui::RichText::new(dates.when(commit.time)).weak());
                }
                Err(error) => put(ui, text, egui::RichText::new(error.to_string()).color(ui.visuals().error_fg_color)),
            }
            if response.clicked() {
                actions.push(Action::Select(Selection::Commit(id)));
            }
            drag_and_drop(ui, &response, history, (index, id), actions);
            if response.double_clicked()
                && let Some(branch) = labels.iter().find(|l| l.kind == RefKind::Local)
            {
                actions.push(Action::Run(Op::Checkout(branch.name.clone())));
            }
            response.context_menu(|ui| commit_menu(ui, id, labels, current.as_deref(), actions));
        }
    });
}

/// The widths of the columns of the graph: the branches and tags, the lanes, the author (none when
/// there is no room for it) and the date. The message has the rest.
#[derive(Debug, Clone, Copy)]
struct Columns {
    width: f32,
    labels: f32,
    lanes: f32,
    author: f32,
    date: f32,
}

impl Columns {
    /// As wide as the user made them, or else as they start: the branches at most a fifth of the
    /// width, the graph as wide as its `lanes`. Neither gets more than a third, and the author goes
    /// before the message gets narrower than it may.
    fn new(ui: &Ui, lanes: usize) -> Self {
        let width = ui.available_width();
        let [labels, graph, author] = RESIZABLE.map(|(name, _)| ui.data_mut(|d| d.get_persisted(width_of(name))));
        let labels = labels.unwrap_or(LABELS.min(width / 5.0)).min(width / 3.0);
        let lanes = graph.unwrap_or(LANE * points(lanes.max(1) + 1)).min(width / 3.0);
        let date = theme::Dates::of(ui.ctx()).width();
        let author = author.unwrap_or(AUTHOR);
        let author = if width - labels - lanes - author - date >= narrowest_message(date) { author } else { 0.0 };
        Self { width, labels, lanes, author, date }
    }
}

/// The narrowest the message gets beside the author: twice the room the author and the `date`
/// start with.
fn narrowest_message(date: f32) -> f32 {
    2.0 * (AUTHOR + date)
}

/// The columns that can be made wider or narrower, with the narrowest they get. egui keeps their
/// widths in its memory, which eframe saves between runs.
const RESIZABLE: [(&str, f32); 3] = [("Branches and tags", 24.0), ("Graph", LANE), ("Author", 40.0)];

/// Where egui keeps the width of the column `name`.
fn width_of(name: &str) -> Id {
    Id::unique(("column width", name))
}

/// The names of the columns, with a border to drag between them to make the one on the left wider
/// or narrower (or the author, on its right); a double click on a border gives the column back the
/// width it starts with. The columns, as wide as they are now.
fn header(ui: &mut Ui, lanes: usize) -> Columns {
    let c = Columns::new(ui, lanes);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(c.width, HEADER), Sense::hover());
    let (graph, message, date) = (c.labels + c.lanes, c.width - c.author - c.date, c.width - c.date);
    let names = [(0.0, c.labels), (c.labels, graph), (graph, message), (message, date), (date, c.width)];
    for (name, (left, right)) in ["Branches and tags", "Graph", "Message", "Author", "Date"].into_iter().zip(names) {
        if right - left > 16.0 {
            let column = Rect::from_x_y_ranges(rect.left() + left + 8.0..=rect.left() + right - 8.0, rect.y_range());
            put(ui, column, RichText::new(name).small().weak());
        }
    }
    let third = c.width / 3.0;
    // Each with its side that stays put, the way it grows from there, its width and the widest it gets.
    let borders = [
        (0.0, 1.0, c.labels, third),
        (c.labels, 1.0, c.lanes, third),
        (date, -1.0, c.author, date - graph - narrowest_message(c.date)),
    ];
    for ((name, narrowest), (from, grows, width, widest)) in RESIZABLE.into_iter().zip(borders) {
        if width <= 0.0 {
            continue;
        }
        let x = rect.left() + from + grows * width;
        let area = Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::new(8.0, HEADER));
        let border = ui.interact(area, Id::unique(("border", name)), Sense::click_and_drag());
        border.widget_info(|| WidgetInfo::labeled(Role::Splitter, true, format!("{name} column width")));
        let border = border.on_hover_cursor(CursorIcon::ResizeHorizontal);
        let visuals = &ui.visuals().widgets;
        let stroke = if border.hovered() || border.dragged() {
            visuals.active.fg_stroke
        } else {
            visuals.noninteractive.bg_stroke
        };
        ui.painter().vline(x, rect.y_range(), stroke);
        if border.double_clicked() {
            ui.data_mut(|d| d.remove::<f32>(width_of(name)));
        } else if border.dragged()
            && let Some(pointer) = border.interact_pointer_pos()
        {
            let width = grows * (pointer.x - rect.left() - from);
            ui.data_mut(|d| d.insert_persisted(width_of(name), width.min(widest).max(narrowest)));
        }
    }
    Columns::new(ui, lanes)
}

/// The up and down arrows move the selection, unless a text field has the keyboard.
fn arrow_keys(ui: &Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let step = ui.input(|i| isize::from(i.key_pressed(Key::ArrowDown)) - isize::from(i.key_pressed(Key::ArrowUp)));
    let next = ws.selected_row().map_or(0, |row| row.saturating_add_signed(step));
    if step != 0
        && let Some(selection) = ws.row_selection(next)
    {
        actions.push(Action::Reveal(selection));
    }
}

/// The search field, with the way from match to match and how many there are.
fn search(ui: &mut Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
    let search = &ws.search;
    ui.horizontal(|ui| {
        let mut text = search.text.clone();
        let hint = format!("Search commits, branches and tags  ({})", ui.ctx().format_shortcut(&FIND));
        let field = TextEdit::singleline(&mut text).hint_text(hint).return_key(None).desired_width(320.0);
        let help = "A message, author, SHA, branch or tag. Enter goes to the next match, Shift+Enter back.";
        let field = ui.add(field).accessible_name("Search").on_hover_text(help);
        if ui.input_mut(|i| i.consume_shortcut(&FIND)) {
            field.request_focus();
        }
        // Escape takes the focus away before the frame, so the field has just lost it.
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            text.clear();
        }
        if text != search.text {
            actions.push(Action::Search(text));
        }
        if field.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            actions.push(Action::Next(!ui.input(|i| i.modifiers.shift)));
        }
        if search.text.is_empty() {
            return;
        }
        let found = !search.rows.is_empty();
        for (arrow, name, down) in [("↑", "Previous match", false), ("↓", "Next match", true)] {
            let button = ui.add_enabled(found, Button::new(RichText::new(arrow).monospace()));
            button.widget_info(|| WidgetInfo::labeled(Role::Button, found, name));
            if button.on_hover_text(name).clicked() {
                actions.push(Action::Next(down));
            }
        }
        ui.weak(search.count(ws.selected_commit(), ws.snapshot.history.ids.len()));
    });
}

/// Lets the commit `id` in row `index` be dragged, and takes a commit dropped onto it to its place,
/// with a line on the side where it lands.
fn drag_and_drop(
    ui: &Ui,
    row: &Response,
    history: &History,
    (index, id): (usize, ObjectId),
    actions: &mut Vec<Action>,
) {
    row.dnd_set_drag_payload(id);
    if let Some(from) = row.dnd_hover_payload::<ObjectId>().filter(|from| **from != id) {
        let y = if history.row(*from) < Some(index) { row.rect.bottom() } else { row.rect.top() };
        ui.painter().hline(row.rect.x_range(), y, ui.visuals().selection.stroke);
    }
    if let Some(from) = row.dnd_release_payload::<ObjectId>().filter(|from| **from != id) {
        actions.push(Action::Move(*from, id));
    }
}

/// Draws one line of text in `rect`, cut off with an ellipsis when it does not fit.
pub fn put(ui: &Ui, rect: Rect, text: impl Into<egui::WidgetText>) {
    let galley = text.into().into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.width(), egui::TextStyle::Body);
    let at = rect.left_center() - Vec2::Y * galley.size().y / 2.0;
    ui.painter().with_clip_rect(rect).galley(at, galley, ui.visuals().text_color());
}

/// A grey dashed line, for the uncommitted changes.
fn dashed(painter: &Painter, from: Pos2, to: Pos2) {
    painter.extend(Shape::dashed_line(&[from, to], Stroke::new(2.0, Color32::GRAY), 3.0, 3.0));
}

/// Draws the lanes and the commit of one row; `HEAD` gets a ring, joined to the changes above it.
fn paint_row(
    painter: &Painter,
    row: &Row,
    lane_x: impl Fn(usize) -> f32,
    merge: bool,
    (head, wip_above): (bool, bool),
) {
    let rect = painter.clip_rect();
    let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());
    let colours = theme::colours(painter.ctx().theme());
    for edge in &row.edges {
        let (y0, y1) = if edge.half == Half::Top { (top, mid) } else { (mid, bottom) };
        let (a, b) = (Pos2::new(lane_x(edge.from), y0), Pos2::new(lane_x(edge.to), y1));
        let stroke = Stroke::new(2.0, colours.lane(edge.colour));
        if edge.from == edge.to {
            painter.line_segment([a, b], stroke);
        } else {
            let curve = [a, Pos2::new(a.x, b.y), Pos2::new(b.x, a.y), b];
            painter.add(CubicBezierShape::from_points_stroke(curve, false, Color32::TRANSPARENT, stroke));
        }
    }
    let node = Pos2::new(lane_x(row.lane), mid);
    painter.circle_filled(node, if merge { 4.0 } else { 6.0 }, colours.lane(row.colour));
    if head {
        painter.circle_stroke(node, 9.0, Stroke::new(1.5, strong(painter)));
    }
    if wip_above {
        dashed(painter, Pos2::new(node.x, top), node - Vec2::Y * 9.0);
    }
}

/// Draws the first branch or tag of a commit as a pill, and how many more there are.
fn paint_labels(painter: &Painter, rect: Rect, labels: &[Ref], colour: Color32, current: Option<&str>) {
    let Some(first) = labels.iter().find(|l| Some(l.name.as_str()) == current).or(labels.first()) else { return };
    let more = if labels.len() > 1 { format!("  +{}", labels.len() - 1) } else { String::new() };
    let (font, text) = (FontId::proportional(12.0), strong(painter));
    let galley = painter.layout_no_wrap(format!("{}{more}", first.name), font, text);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let pill = Rect::from_min_size(Pos2::new(rect.left() + 6.0, rect.center().y - size.y / 2.0), size);
    let pill = pill.intersect(rect.shrink2(Vec2::new(4.0, 0.0)));
    match first.kind {
        RefKind::Local => painter.rect_filled(pill, 4.0, colour.gamma_multiply(0.55)),
        RefKind::Remote => painter.rect_stroke(pill, 4.0, Stroke::new(1.0, colour), egui::StrokeKind::Inside),
        RefKind::Tag => painter.rect_filled(pill, 0.0, theme::colours(painter.ctx().theme()).tag),
    };
    if Some(first.name.as_str()) == current {
        painter.rect_stroke(pill, 4.0, Stroke::new(1.5, text), egui::StrokeKind::Outside);
    }
    let line = Pos2::new(pill.right(), rect.center().y);
    painter.line_segment([line, Pos2::new(rect.right(), line.y)], Stroke::new(1.0, colour.gamma_multiply(0.6)));
    painter.with_clip_rect(pill).galley(pill.min + Vec2::new(7.0, 3.0), galley, text);
}

/// The colour of what stands out most, as the commit checked out: white on dark, black on light.
fn strong(painter: &Painter) -> Color32 {
    painter.ctx().global_style().visuals.strong_text_color()
}

/// The right-click menu of a commit: what can be done with each branch and tag on it, then with
/// the commit itself.
fn commit_menu(ui: &mut Ui, id: ObjectId, labels: &[Ref], current: Option<&str>, actions: &mut Vec<Action>) {
    for label in labels {
        menu(ui, ref_items(label, current), actions);
        ui.separator();
    }
    menu(ui, commit_items(id, current), actions);
}

/// Menu items, each closing the menu with its action; the items of a submenu have its name and
/// ` › ` before their own.
pub fn menu(ui: &mut Ui, items: Vec<(String, Action)>, actions: &mut Vec<Action>) {
    let mut items = items.into_iter().peekable();
    while let Some((name, action)) = items.next() {
        let Some((submenu, item)) = name.split_once(" › ") else {
            if ui.button(&name).clicked() {
                actions.push(action);
                ui.close();
            }
            continue;
        };
        let mut inside = vec![(item.to_owned(), action)];
        let prefix = format!("{submenu} › ");
        while let Some((name, action)) = items.next_if(|(name, _)| name.starts_with(&prefix)) {
            inside.push((name[prefix.len()..].to_owned(), action));
        }
        ui.menu_button(submenu, |ui| menu(ui, inside, actions));
    }
}

/// What can be done with a commit; the resets in a submenu.
pub fn commit_items(id: ObjectId, current: Option<&str>) -> Vec<(String, Action)> {
    let sha = &id.to_string();
    let items = [
        ("Checkout this commit", Action::Run(Op::Checkout(sha.to_owned()))),
        ("Create branch here…", Action::Prompt(Prompt::Branch { at: sha.to_owned() })),
        ("Create tag here…", Action::Prompt(Prompt::Tag { at: sha.to_owned() })),
        ("Cherry-pick commit", Action::Run(Op::CherryPick(sha.to_owned()))),
        ("Revert commit", Action::Run(Op::Revert(sha.to_owned()))),
        ("Reword commit…", Action::Reword(id)),
        ("Interactive rebase onto this commit…", Action::Plan(sha.to_owned())),
    ];
    let mut items: Vec<_> = items.into_iter().map(|(name, action)| (name.to_owned(), action)).collect();
    for (text, mode) in [
        ("Soft: keep all changes", ResetMode::Soft),
        ("Mixed: keep working copy", ResetMode::Mixed),
        ("Hard: discard all changes", ResetMode::Hard),
    ] {
        let op = Op::Reset { to: sha.to_owned(), mode };
        let action = if mode == ResetMode::Hard {
            let question = format!(
                "Hard reset {} to {}, throwing away every uncommitted change?",
                current.unwrap_or("HEAD"),
                &sha[..7]
            );
            Action::Confirm(Confirm { question, verb: "Reset", op })
        } else {
            Action::Run(op)
        };
        items.push((format!("Reset current branch to this commit › {text}"), action));
    }
    items.push(("Copy commit SHA".to_owned(), Action::Copy(sha.to_owned())));
    items
}

/// What can be done with a branch or tag.
pub fn ref_items(label: &Ref, current: Option<&str>) -> Vec<(String, Action)> {
    let name = &label.name;
    let into = current.unwrap_or("HEAD");
    match label.kind {
        RefKind::Tag => vec![(format!("Delete tag {name}"), Action::Run(Op::DeleteTag(name.clone())))],
        _ if Some(name.as_str()) == current => Vec::new(),
        kind => {
            let local =
                if kind == RefKind::Remote { name.split_once('/').map_or(name.as_str(), |(_, b)| b) } else { name };
            let mut items = vec![
                (format!("Checkout {name}"), Action::Run(Op::Checkout(local.to_owned()))),
                (format!("Merge {name} into {into}"), Action::Run(Op::Merge(name.clone()))),
                (format!("Rebase {into} onto {name}"), Action::Run(Op::Rebase(name.clone()))),
            ];
            if kind == RefKind::Local {
                items.push((format!("Delete {name}"), Action::DeleteBranch(name.clone())));
            }
            items
        }
    }
}
