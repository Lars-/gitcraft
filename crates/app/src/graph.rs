//! The commit graph: one row per commit, with its branches, lanes and message.

use eframe::egui::epaint::CubicBezierShape;
use eframe::egui::{self, Color32, FontId, Key, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use gitcraft_core::git::{Op, ResetMode};
use gitcraft_core::graph::{Half, Row};
use gitcraft_core::repo::{Label as Ref, RefKind};

use crate::app::{Action, Prompt, Selection, Workspace};
use crate::theme;

/// The height of a row.
pub const ROW: f32 = 28.0;
const LANE: f32 = 16.0;
const LABELS: f32 = 170.0;
const AUTHOR: f32 = 140.0;
const DATE: f32 = 100.0;

/// A count as a distance in points.
fn points(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// Shows the graph.
pub fn show(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let wip = usize::from(!ws.snapshot.status.is_clean());
    let total = ws.snapshot.history.ids.len() + wip;
    if !ui.ctx().egui_wants_keyboard_input() {
        let step = ui.input(|i| isize::from(i.key_pressed(Key::ArrowDown)) - isize::from(i.key_pressed(Key::ArrowUp)));
        let next = ws.selected_row().map_or(0, |row| row.saturating_add_signed(step));
        if step != 0
            && let Some(selection) = ws.row_selection(next)
        {
            actions.push(Action::Reveal(selection));
        }
    }
    let mut area = egui::ScrollArea::vertical().auto_shrink(false);
    if let Some(row) = ws.scroll_to.take()
        && !ws.visible.contains(&row)
    {
        area = area.vertical_scroll_offset(points(row.saturating_sub(5)) * ROW);
    }
    let selected = ws.selected_row();
    let current = ws.snapshot.status.branch.clone();
    let now = theme::now();
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_rows(ui, ROW, total, |ui, range| {
        ws.visible = range.clone();
        let history = &ws.snapshot.history;
        // The graph gets at most a third of the width; the message never less than a third.
        let width = ui.available_width();
        let lanes = (LANE * points(history.graph.width().max(1) + 1)).min(width / 3.0);
        let labels_width = LABELS.min(width / 5.0);
        let (author_width, date_width) =
            if width - labels_width - lanes > 3.0 * (AUTHOR + DATE) { (AUTHOR, DATE) } else { (0.0, DATE) };
        let first = range.start.saturating_sub(wip);
        let rows = history.graph.rows(first..range.end.saturating_sub(wip));
        let head_lane = history.head.and_then(|head| history.graph.rows(head..head + 1).pop()).map_or(0, |r| r.lane);
        for i in range {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
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
            let colour = theme::lane(row.lane);
            paint_labels(&painter, rect.with_max_x(rect.left() + labels_width), labels, colour, current.as_deref());
            match ws.commits.get(&ws.repo, id) {
                Ok(commit) => {
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, commit.summary()));
                    put(ui, text, commit.summary());
                    if author_width > 0.0 {
                        let author =
                            Rect::from_x_y_ranges(text.right() + 8.0..=text.right() + author_width, rect.y_range());
                        put(ui, author, egui::RichText::new(&commit.author).weak());
                    }
                    let date = Rect::from_x_y_ranges(rect.right() - date_width..=rect.right() - 8.0, rect.y_range());
                    put(ui, date, egui::RichText::new(theme::when(commit.time, now)).weak());
                }
                Err(error) => put(ui, text, egui::RichText::new(error.to_string()).color(theme::ERROR)),
            }
            if response.clicked() {
                actions.push(Action::Select(Selection::Commit(id)));
            }
            if response.double_clicked()
                && let Some(branch) = labels.iter().find(|l| l.kind == RefKind::Local)
            {
                actions.push(Action::Run(Op::Checkout(branch.name.clone())));
            }
            response.context_menu(|ui| commit_menu(ui, &id.to_string(), labels, current.as_deref(), actions));
        }
    });
}

/// Draws one line of text in `rect`, cut off with an ellipsis when it does not fit.
fn put(ui: &Ui, rect: Rect, text: impl Into<egui::WidgetText>) {
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
    for edge in &row.edges {
        let (y0, y1, colour) = match edge.half {
            Half::Top => (top, mid, theme::lane(edge.from)),
            Half::Bottom => (mid, bottom, theme::lane(edge.to)),
        };
        let (a, b) = (Pos2::new(lane_x(edge.from), y0), Pos2::new(lane_x(edge.to), y1));
        let stroke = Stroke::new(2.0, colour);
        if edge.from == edge.to {
            painter.line_segment([a, b], stroke);
        } else {
            let curve = [a, Pos2::new(a.x, b.y), Pos2::new(b.x, a.y), b];
            painter.add(CubicBezierShape::from_points_stroke(curve, false, Color32::TRANSPARENT, stroke));
        }
    }
    let node = Pos2::new(lane_x(row.lane), mid);
    painter.circle_filled(node, if merge { 4.0 } else { 6.0 }, theme::lane(row.lane));
    if head {
        painter.circle_stroke(node, 9.0, Stroke::new(1.5, Color32::WHITE));
    }
    if wip_above {
        dashed(painter, Pos2::new(node.x, top), node - Vec2::Y * 9.0);
    }
}

/// Draws the first branch or tag of a commit as a pill, and how many more there are.
fn paint_labels(painter: &Painter, rect: Rect, labels: &[Ref], colour: Color32, current: Option<&str>) {
    let Some(first) = labels.iter().find(|l| Some(l.name.as_str()) == current).or(labels.first()) else { return };
    let more = if labels.len() > 1 { format!("  +{}", labels.len() - 1) } else { String::new() };
    let font = FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(format!("{}{more}", first.name), font, Color32::WHITE);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let pill = Rect::from_min_size(Pos2::new(rect.left() + 6.0, rect.center().y - size.y / 2.0), size);
    let pill = pill.intersect(rect.shrink2(Vec2::new(4.0, 0.0)));
    match first.kind {
        RefKind::Local => painter.rect_filled(pill, 4.0, colour.gamma_multiply(0.55)),
        RefKind::Remote => painter.rect_stroke(pill, 4.0, Stroke::new(1.0, colour), egui::StrokeKind::Inside),
        RefKind::Tag => painter.rect_filled(pill, 0.0, Color32::from_gray(70)),
    };
    if Some(first.name.as_str()) == current {
        painter.rect_stroke(pill, 4.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
    }
    let line = Pos2::new(pill.right(), rect.center().y);
    painter.line_segment([line, Pos2::new(rect.right(), line.y)], Stroke::new(1.0, colour.gamma_multiply(0.6)));
    painter.with_clip_rect(pill).galley(pill.min + Vec2::new(7.0, 3.0), galley, Color32::WHITE);
}

/// The right-click menu of a commit.
pub fn commit_menu(ui: &mut Ui, sha: &str, labels: &[Ref], current: Option<&str>, actions: &mut Vec<Action>) {
    let mut item = |ui: &mut Ui, text: &str, action: Action| {
        if ui.button(text).clicked() {
            actions.push(action);
            ui.close();
        }
    };
    for label in labels {
        ref_items(ui, label, current, &mut item);
        ui.separator();
    }
    item(ui, "Checkout this commit", Action::Run(Op::Checkout(sha.to_owned())));
    item(ui, "Create branch here…", Action::Prompt(Prompt::Branch { at: sha.to_owned() }));
    item(ui, "Create tag here…", Action::Prompt(Prompt::Tag { at: sha.to_owned() }));
    item(ui, "Cherry-pick commit", Action::Run(Op::CherryPick(sha.to_owned())));
    item(ui, "Revert commit", Action::Run(Op::Revert(sha.to_owned())));
    ui.menu_button("Reset current branch to this commit", |ui| {
        for (text, mode) in [
            ("Soft: keep all changes", ResetMode::Soft),
            ("Mixed: keep working copy", ResetMode::Mixed),
            ("Hard: discard all changes", ResetMode::Hard),
        ] {
            item(ui, text, Action::Run(Op::Reset { to: sha.to_owned(), mode }));
        }
    });
    if ui.button("Copy commit SHA").clicked() {
        ui.ctx().copy_text(sha.to_owned());
        ui.close();
    }
}

/// The menu items for a branch or tag.
pub fn ref_items(ui: &mut Ui, label: &Ref, current: Option<&str>, item: &mut impl FnMut(&mut Ui, &str, Action)) {
    let name = &label.name;
    let into = current.unwrap_or("HEAD");
    match label.kind {
        RefKind::Tag => item(ui, &format!("Delete tag {name}"), Action::Run(Op::DeleteTag(name.clone()))),
        _ if Some(name.as_str()) == current => {}
        kind => {
            let local =
                if kind == RefKind::Remote { name.split_once('/').map_or(name.as_str(), |(_, b)| b) } else { name };
            item(ui, &format!("Checkout {name}"), Action::Run(Op::Checkout(local.to_owned())));
            item(ui, &format!("Merge {name} into {into}"), Action::Run(Op::Merge(name.clone())));
            item(ui, &format!("Rebase {into} onto {name}"), Action::Run(Op::Rebase(name.clone())));
            if kind == RefKind::Local {
                item(ui, &format!("Delete {name}"), Action::Run(Op::DeleteBranch(name.clone())));
            }
        }
    }
}
