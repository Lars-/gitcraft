//! The whole app, driven through its accessibility tree against real repositories.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use gitcraft_core::askpass;
use gitcraft_core::diff::Source;
use gitcraft_core::git::{Git, Op};
use tempfile::TempDir;

use crate::app::{Action, Selection, Versions};
use crate::commands;
use crate::start::percent;
use crate::theme;
use crate::ui::{App, KEYS};

/// A repository with one commit, with the tester's own git config kept out of the way.
fn repo() -> (TempDir, Git) {
    let dir = tempfile::tempdir().unwrap();
    let git = Git::new(fs::canonicalize(dir.path()).unwrap());
    git.run(&["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [
        ("user.name", "Tess Ter"),
        ("user.email", "tess@example.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git.run(&["config", key, value]).unwrap();
    }
    commit(&git, "First commit");
    (dir, git)
}

fn commit(git: &Git, message: &str) {
    fs::write(git.dir().join("file.txt"), message).unwrap();
    git.apply(&Op::Stage(vec!["file.txt".into()])).unwrap();
    git.apply(&Op::Commit { message: message.into(), amend: false }).unwrap();
}

fn log(git: &Git) -> String {
    git.run(&["log", "--format=%s"]).unwrap()
}

fn app(path: Option<&Path>) -> Harness<'static, App> {
    run(App::restore(|_| None, path.map(Path::to_path_buf)))
}

fn run(app: App) -> Harness<'static, App> {
    let mut harness =
        Harness::builder().with_size([1280.0, 720.0]).build_ui_state(|ui, app: &mut App| app.show(ui), app);
    settle(&mut harness);
    harness
}

/// Steps the app until its background job is done and the frame is stable, also when the
/// watcher starts a reload in the meantime (macOS reports changes late).
fn settle(harness: &mut Harness<'_, App>) {
    let start = Instant::now();
    harness.step();
    while busy(harness.state()) || harness.try_run().is_err() {
        assert!(start.elapsed() < Duration::from_secs(30), "the app stays busy");
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
}

/// Steps the app until `shown`, as the watcher wakes it.
fn wait_until(harness: &mut Harness<'_, App>, what: &str, shown: impl Fn(&Harness<'_, App>) -> bool) {
    let start = Instant::now();
    while !shown(harness) {
        assert!(start.elapsed() < Duration::from_secs(10), "{what} never showed up");
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
}

fn busy(app: &App) -> bool {
    app.tabs.iter().any(|ws| ws.busy().is_some()) || app.start.dialog.as_ref().is_some_and(crate::start::Dialog::busy)
}

/// The names on the tabs.
fn tabs(app: &App) -> Vec<String> {
    app.tabs.iter().map(|ws| ws.repo.name()).collect()
}

fn name(git: &Git) -> String {
    git.dir().file_name().unwrap().to_string_lossy().into_owned()
}

/// Opens `git` from the start screen of a new tab.
fn open_in_new_tab(harness: &mut Harness<'_, App>, git: &Git) {
    harness.get_by_label("New tab").click();
    harness.run();
    harness.get_by_role(Role::TextInput).click();
    harness.run();
    harness.get_by_role(Role::TextInput).type_text(git.dir().to_str().unwrap());
    harness.run();
    harness.key_press(Key::Enter);
    settle(harness);
}

/// Types `text` over what the text field labelled `label` holds.
fn type_into(harness: &mut Harness<'_, App>, label: &str, text: &str) {
    harness.get_by_label(label).click();
    settle(harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label(label).type_text(text);
    settle(harness);
}

#[test]
fn the_start_screen_offers_to_open_a_repository() {
    let harness = app(None);
    harness.get_by_label("Open a repository…");
}

#[test]
fn a_bad_path_explains_itself_on_the_start_screen() {
    let harness = app(Some(Path::new("/surely/not/a/repository")));
    assert!(harness.state().workspace().is_none());
    harness.get_by_label_contains("/surely/not/a/repository");
}

#[test]
fn clone_a_repository_from_the_start_screen() {
    let (source_dir, _source) = repo();
    let into = tempfile::tempdir().unwrap();
    let mut harness = app(None);
    harness.get_by_label("Clone a repository…").click();
    harness.run();
    type_into(&mut harness, "URL", source_dir.path().to_str().unwrap());
    type_into(&mut harness, "Folder", into.path().to_str().unwrap());
    harness.get_by_label("Clone").click();
    settle(&mut harness);

    let ws = harness.state().workspace().expect("the clone is open");
    let name = source_dir.path().file_name().unwrap();
    assert_eq!(fs::canonicalize(ws.repo.git.dir()).unwrap(), fs::canonicalize(into.path().join(name)).unwrap());
    harness.get_by_role_and_label(Role::Button, "First commit");
}

#[test]
fn a_failing_clone_shows_git_s_message_and_stays_open() {
    let into = tempfile::tempdir().unwrap();
    let mut harness = app(None);
    harness.get_by_label("Clone a repository…").click();
    harness.run();
    type_into(&mut harness, "URL", into.path().join("missing").to_str().unwrap());
    type_into(&mut harness, "Folder", into.path().join("clones").to_str().unwrap());
    harness.get_by_label("Clone").click();
    settle(&mut harness);

    assert!(harness.state().workspace().is_none());
    let error = harness.state().start.dialog.as_ref().and_then(|d| d.error.clone()).expect("an error");
    harness.get_by_label(&error);
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.state().start.dialog.is_none());
}

#[test]
fn create_a_repository_from_the_start_screen() {
    let into = tempfile::tempdir().unwrap();
    let mut harness = app(None);
    harness.get_by_label("Create a repository…").click();
    harness.run();
    type_into(&mut harness, "Name", "project");
    type_into(&mut harness, "Folder", into.path().to_str().unwrap());
    harness.get_by_label("Create").click();
    settle(&mut harness);

    assert_eq!(harness.state().workspace().expect("the new repository is open").repo.name(), "project");
    assert!(into.path().join("project/.git").is_dir());
}

#[test]
fn git_s_progress_lines_fill_the_progress_bar() {
    assert_eq!(percent("Receiving objects:  50% (500/1000), 1.20 MiB | 2.00 MiB/s"), Some(0.5));
    assert_eq!(percent("remote: Counting objects: 100% (9/9), done."), Some(1.0));
    assert_eq!(percent("Cloning into 'repo'..."), None);
}

#[test]
fn what_git_asks_is_asked_in_a_dialog_and_answered() {
    let (address, questions) = askpass::listen(|| {}).unwrap();
    let mut app = App::restore(|_| None, None);
    app.questions = Some(questions);
    let mut harness = run(app);
    let ask = |prompt: &'static str| {
        let address = address.clone();
        std::thread::spawn(move || askpass::ask(&address, prompt).unwrap())
    };

    let username = ask("Username for 'https://example.com': ");
    wait_until(&mut harness, "the question", |h| h.query_by_label_contains("Username for").is_some());
    let password = ask("Password for 'https://tess@example.com': ");
    harness.get_by_label("Username for 'https://example.com':").type_text("tess");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(username.join().unwrap().as_deref(), Some("tess"));

    // The next question waited its turn; a password is hidden, and Escape answers nothing.
    wait_until(&mut harness, "the next question", |h| h.query_by_label_contains("Password for").is_some());
    let field = harness.get_by_label("Password for 'https://tess@example.com':");
    assert_eq!(field.accesskit_node().role(), Role::PasswordInput);
    field.type_text("secret");
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert_eq!(password.join().unwrap(), None);
    assert!(harness.query_by_label_contains("Password for").is_none());
}

#[test]
fn stage_and_commit_a_change() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_role_and_label(Role::Button, "First commit");

    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("Stage").click();
    settle(&mut harness);
    harness.state_mut().workspace_mut().unwrap().summary = "Second commit".into();
    harness.run();
    harness.get_by_label("Commit changes to 1 file").click();
    settle(&mut harness);

    assert_eq!(log(&git), "Second commit\nFirst commit\n");
    harness.get_by_role_and_label(Role::Button, "Second commit");
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn undo_a_commit_and_redo_it() {
    let (_dir, git) = repo();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));

    harness.get_by_label("Undo").click();
    settle(&mut harness);
    assert_eq!(log(&git), "First commit\n");
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("Unstage");
    harness.get_by_label("Redo").click();
    settle(&mut harness);
    assert_eq!(log(&git), "Second commit\nFirst commit\n");
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn a_selected_commit_that_left_the_graph_gives_way_to_head() {
    let (_dir, git) = repo();
    let first = gitcraft_core::ObjectId::from_hex(rev(&git, "HEAD").trim().as_bytes()).unwrap();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));
    assert_eq!(harness.get_all_by_label("Second commit").count(), 2, "in the graph and in the details");

    git.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    wait_until(&mut harness, "the details of HEAD", |harness| {
        harness.state().workspace().unwrap().selection == Some(Selection::Commit(first))
    });
    assert!(harness.query_by_label("Second commit").is_none(), "neither in the graph nor in the details");
}

fn read(git: &Git, path: &str) -> String {
    fs::read_to_string(git.dir().join(path)).unwrap()
}

#[test]
fn discard_a_changed_file_once_confirmed() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);

    harness.get_by_label("Discard file.txt").click();
    settle(&mut harness);
    harness.get_by_label("Discard the unstaged changes to file.txt?");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "changed");

    harness.get_by_label("Discard file.txt").click();
    settle(&mut harness);
    harness.get_by_label("Discard").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "First commit");
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn discarding_an_open_untracked_file_deletes_it_and_closes_it() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("new.txt"), "new").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("new.txt").click();
    settle(&mut harness);
    assert!(harness.state().workspace().unwrap().open_file.is_some());

    harness.get_by_label("Discard new.txt").click();
    settle(&mut harness);
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert!(!git.dir().join("new.txt").exists());
    let ws = harness.state().workspace().unwrap();
    assert!(ws.open_file.is_none(), "the diff of the deleted file closes");
    assert_eq!(ws.error, None);
}

/// The parts of the text on screen that are painted on `background`.
fn painted_on(harness: &Harness<'_, App>, background: egui::Color32) -> Vec<String> {
    let jobs = harness.output().shapes.iter().filter_map(|clipped| match &clipped.shape {
        egui::Shape::Text(text) => Some(&text.galley.job),
        _ => None,
    });
    jobs.flat_map(|job| {
        let marked = job.sections.iter().filter(|section| section.format.background == background);
        marked.map(|section| job.text[section.byte_range.start.0..section.byte_range.end.0].to_owned())
    })
    .collect()
}

#[test]
fn a_changed_line_marks_the_words_that_changed() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "let total = price * count;\nkeep\nold line\n").unwrap();
    git.run(&["commit", "-qam", "Total"]).unwrap();
    fs::write(git.dir().join("file.txt"), "let total = price * amount;\nkeep\nsomething else\n").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);

    // Only the word that changed stands out; a line rewritten as a whole has nothing to point at.
    assert_eq!(painted_on(&harness, theme::DARK.removed_word), ["count"]);
    assert_eq!(painted_on(&harness, theme::DARK.added_word), ["amount"]);
}

/// Where the lines on screen that hold `text` are painted, top to bottom.
fn painted_at(harness: &Harness<'_, App>, text: &str) -> Vec<egui::Pos2> {
    let shapes = harness.output().shapes.iter();
    shapes
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text.contains(text) => Some(shape.pos),
            _ => None,
        })
        .collect()
}

/// Whether two coordinates on screen are the same, to the pixel.
fn same(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0
}

/// Clicks with the mouse at `at`.
fn click_at(harness: &mut Harness<'_, App>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.drag_at(at);
    harness.drop_at(at);
    settle(harness);
}

#[test]
fn a_diff_side_by_side_puts_old_and_new_next_to_each_other_to_stage_from() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "let total = price * count;\nkeep\n").unwrap();
    git.run(&["commit", "-qam", "Total"]).unwrap();
    fs::write(git.dir().join("file.txt"), "let total = price * amount;\nkeep\nnew line\n").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);
    let [old] = painted_at(&harness, "- let")[..] else { panic!("no old line") };
    let [new] = painted_at(&harness, "+ let")[..] else { panic!("no new line") };
    assert!(same(old.x, new.x) && old.y < new.y, "one under the other at first");

    harness.get_by_label("Side by side").click();
    settle(&mut harness);
    let [old] = painted_at(&harness, "- let")[..] else { panic!("no old line") };
    let [new] = painted_at(&harness, "+ let")[..] else { panic!("no new line") };
    assert!(same(old.y, new.y) && old.x < new.x, "the old line faces the new one");
    let [extra] = painted_at(&harness, "+ new line")[..] else { panic!("no added line") };
    assert!(same(extra.x, new.x) && extra.y > new.y, "a line that pairs with none is on its own side");
    let [left, right] = painted_at(&harness, "  keep")[..] else { panic!("not on both sides") };
    assert!(same(left.y, right.y) && same(left.x, old.x) && same(right.x, new.x));
    assert_eq!(painted_on(&harness, theme::DARK.removed_word), ["count"]);
    assert_eq!(painted_on(&harness, theme::DARK.added_word), ["amount"]);

    // Either side's line can be picked.
    click_at(&mut harness, old);
    click_at(&mut harness, new);
    harness.get_by_label("Stage 2 lines").click();
    settle(&mut harness);
    assert_eq!(git.run(&["show", ":file.txt"]).unwrap(), "let total = price * amount;\nkeep\n");

    // The next diff, of what is still unstaged, is side by side too, until it is turned off.
    harness.get_by_label("Close").click();
    settle(&mut harness);
    harness.get_all_by_label("file.txt").next().unwrap().click();
    settle(&mut harness);
    let [_, keep] = painted_at(&harness, "  keep")[..] else { panic!("not side by side") };
    assert!(keep.x > old.x);
    harness.get_by_label("Side by side").click();
    settle(&mut harness);
    assert_eq!(painted_at(&harness, "  keep").len(), 1);
}

/// A PNG of `width` by `height` pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = std::io::Cursor::new(Vec::new());
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 40, 40, 255]));
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    png.into_inner()
}

/// The sizes of the pictures on screen, left to right.
fn pictures_painted(harness: &Harness<'_, App>) -> Vec<egui::Vec2> {
    let shapes = harness.output().shapes.iter().map(|clipped| &clipped.shape);
    let pictures = shapes.filter(|shape| shape.texture_id() != egui::TextureId::default());
    pictures.map(|shape| shape.visual_bounding_rect().size()).collect()
}

/// Opens the diff of `path` in the details and waits for its old and new version.
fn open_versions(harness: &mut Harness<'_, App>, path: &str) {
    harness.get_by_label(path).click();
    let read = |h: &Harness<'_, App>| {
        let file = h.state().workspace().unwrap().open_file.as_ref();
        file.is_some_and(|file| file.source.path() == path && file.reading.is_none())
    };
    wait_until(harness, "the versions", read);
    settle(harness);
}

#[test]
fn a_picture_shows_its_old_and_its_new_version_side_by_side() {
    let (_dir, git) = repo();
    let (old, new) = (png(3, 2), png(40, 30));
    fs::write(git.dir().join("logo.png"), &old).unwrap();
    git.apply(&Op::Stage(vec!["logo.png".into()])).unwrap();
    git.apply(&Op::Commit { message: "Logo".into(), amend: false }).unwrap();
    fs::write(git.dir().join("logo.png"), &new).unwrap();
    fs::write(git.dir().join("broken.png"), [0, 1, 2]).unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);

    open_versions(&mut harness, "logo.png");
    harness.get_by_label(&format!("Old · 3 × 2 · {} bytes", old.len()));
    harness.get_by_label(&format!("New · 40 × 30 · {} bytes", new.len()));
    let sizes = pictures_painted(&harness);
    assert_eq!(sizes.len(), 2, "{sizes:?}");
    assert!(same(sizes[0].x, 3.0) && same(sizes[0].y, 2.0) && same(sizes[1].x, 40.0) && same(sizes[1].y, 30.0));

    // A change outside shows at once.
    fs::write(git.dir().join("logo.png"), png(5, 5)).unwrap();
    wait_until(&mut harness, "the new picture", |h| h.query_by_label_contains("New · 5 × 5").is_some());

    open_versions(&mut harness, "broken.png");
    harness.get_by_label("Old · none");
    harness.get_by_label_contains("New · cannot be shown: ");

    // A commit that added it has nothing before.
    harness.get_by_label("Close").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Logo").click();
    settle(&mut harness);
    open_versions(&mut harness, "logo.png");
    harness.get_by_label("Old · none");
    harness.get_by_label(&format!("New · 3 × 2 · {} bytes", old.len()));
    assert_eq!(pictures_painted(&harness).len(), 1);
}

/// The colours of the text on screen that reads `text`.
fn coloured(harness: &Harness<'_, App>, text: &str) -> Vec<egui::Color32> {
    let jobs = harness.output().shapes.iter().filter_map(|clipped| match &clipped.shape {
        egui::Shape::Text(text) => Some(&text.galley.job),
        _ => None,
    });
    jobs.flat_map(|job| {
        let pieces = job.sections.iter().filter(|s| job.text[s.byte_range.start.0..s.byte_range.end.0] == *text);
        pieces.map(|section| section.format.color)
    })
    .collect()
}

#[test]
fn code_in_a_diff_has_the_colours_of_its_language() {
    let (_dir, git) = repo();
    let code = "/*\n * one\n * two\n * three\n * four\n */\nfn main() {}\n";
    fs::write(git.dir().join("main.rs"), code).unwrap();
    git.apply(&Op::Stage(vec!["main.rs".into()])).unwrap();
    git.apply(&Op::Commit { message: "Main".into(), amend: false }).unwrap();
    fs::write(git.dir().join("main.rs"), code.replace("{}", "{ greet(); }")).unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    open_versions(&mut harness, "main.rs");

    // The hunk starts in a comment that starts before it, and it still reads as one.
    let plain = harness.ctx.style_of(egui::Theme::Dark).visuals.text_color();
    let [comment] = coloured(&harness, " * three")[..] else { panic!("no line of the comment") };
    let [keyword, again] = coloured(&harness, "fn")[..] else { panic!("no old and new line") };
    assert!(comment != plain && keyword != plain && keyword != comment && again == keyword);
    assert!(painted_on(&harness, theme::DARK.added_word).concat().contains("greet"), "the words still stand out");

    // A submodule is no code, and its diff shows where it points.
    let head = git.run(&["rev-parse", "HEAD"]).unwrap();
    fs::create_dir(git.dir().join("sub")).unwrap();
    git.run(&["update-index", "--add", "--cacheinfo", &format!("160000,{},sub", head.trim())]).unwrap();
    harness.get_by_label("Refresh").click();
    settle(&mut harness);
    open_versions(&mut harness, "sub");
    assert_eq!(painted_at(&harness, "Subproject commit").len(), 1);
    assert_eq!(harness.state().workspace().unwrap().error, None);
}

#[test]
fn sizes_read_the_way_people_say_them() {
    assert_eq!(crate::diff::bytes(1023), "1023 bytes");
    assert_eq!(crate::diff::bytes(1536), "1.5 KB");
    assert_eq!(crate::diff::bytes(3 << 20), "3.0 MB");
}

#[test]
fn discard_single_lines_and_hunks() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);

    // Pick the line "four", as a click on it does.
    let file = harness.state_mut().workspace_mut().unwrap().open_file.as_mut().unwrap();
    let four = file.diff.hunks[0].lines.iter().position(|l| l.text == "four").unwrap();
    file.lines.insert((0, four));
    // A reload that changes nothing keeps the pick.
    harness.get_by_label("Refresh").click();
    settle(&mut harness);
    harness.get_by_label("Discard 1 line").click();
    settle(&mut harness);
    harness.get_by_label("Discard").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "one\ntwo\nthree\n");

    // Escape closes the question and nothing else.
    harness.get_by_label("Discard hunk").click();
    settle(&mut harness);
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.query_by_label("Discard").is_none());
    assert!(harness.state().workspace().unwrap().open_file.is_some());

    harness.get_by_label("Discard hunk").click();
    settle(&mut harness);
    harness.get_by_label("Discard").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "First commit");
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn what_is_asked_for_while_git_is_busy_runs_next() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    let mut harness = app(Some(git.dir()));
    let ctx = harness.ctx.clone();
    let ws = harness.state_mut().workspace_mut().unwrap();
    ws.apply(&ctx, Action::Refresh);
    ws.apply(&ctx, Action::Run(Op::Stage(vec!["file.txt".into()])));
    ws.apply(&ctx, Action::Run(Op::Commit { message: "Second commit".into(), amend: false }));
    settle(&mut harness);

    assert_eq!(log(&git), "Second commit\nFirst commit\n");
}

#[test]
fn changes_made_outside_show_up_by_themselves() {
    let (_dir, git) = repo();
    let mut harness = app(Some(git.dir()));

    fs::write(git.dir().join("file.txt"), "edited elsewhere").unwrap();
    wait_until(&mut harness, "the edit", |h| h.query_by_label("Uncommitted changes").is_some());
    commit(&git, "Committed elsewhere");
    wait_until(&mut harness, "the commit", |h| {
        h.query_by_role_and_label(Role::Button, "Committed elsewhere").is_some()
    });
    settle(&mut harness);
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn the_graph_scrolls_to_a_commit_just_below_its_edge() {
    let (_dir, git) = repo();
    for n in 2..=40 {
        commit(&git, &format!("Commit {n}"));
    }
    let mut harness = app(Some(git.dir()));
    // egui draws a row or two more than fit in the window; walking onto those scrolls too.
    for n in (2..40).rev() {
        harness.key_press(Key::ArrowDown);
        settle(&mut harness);
        let row = harness.get_by_role_and_label(Role::Button, &format!("Commit {n}")).rect();
        assert!(row.bottom() <= 720.0, "Commit {n} is at {row:?}, below the window");
    }
}

#[test]
fn arrow_keys_walk_the_history() {
    let (_dir, git) = repo();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));
    let ids = harness.state().workspace().unwrap().snapshot.history.ids.clone();
    assert_eq!(harness.state().workspace().unwrap().selection, Some(Selection::Commit(ids[0])));

    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(harness.state().workspace().unwrap().selection, Some(Selection::Commit(ids[1])));
    harness.key_press(Key::ArrowUp);
    harness.run();
    assert_eq!(harness.state().workspace().unwrap().selection, Some(Selection::Commit(ids[0])));
}

#[test]
fn search_the_graph_and_go_from_match_to_match() {
    let (_dir, git) = repo();
    commit(&git, "Fix the parser");
    git.run(&["tag", "release"]).unwrap();
    commit(&git, "Prepare the release");
    commit(&git, "Fix the printer");
    let mut harness = app(Some(git.dir()));
    let ids = harness.state().workspace().unwrap().snapshot.history.ids.clone();
    let selected = |harness: &Harness<'_, App>| match harness.state().workspace().unwrap().selection {
        Some(Selection::Commit(id)) => ids.iter().position(|&i| i == id),
        _ => None,
    };

    // The first match is shown at once; Enter and the arrows go on, round to the start again.
    harness.key_press(Key::ArrowDown);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    settle(&mut harness);
    harness.get_by_label("Search").type_text("FIX");
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(0));
    harness.get_by_label("1 of 2");
    harness.key_press(Key::Enter);
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(2));
    harness.get_by_label("2 of 2");
    harness.key_press(Key::Enter);
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(0));
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(2));
    harness.get_by_label("Previous match").click();
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(0));

    // A tag by its name goes before a message that has it; a commit by its SHA.
    type_into(&mut harness, "Search", "release");
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(2));
    harness.get_by_label("2 of 2");
    type_into(&mut harness, "Search", &ids[3].to_string()[..7]);
    settle(&mut harness);
    assert_eq!(selected(&harness), Some(3));
    type_into(&mut harness, "Search", "nothing like this");
    settle(&mut harness);
    harness.get_by_label("No matches");

    // Escape ends the search, and a commit made meanwhile is searched too.
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert_eq!(harness.state().workspace().unwrap().search.text, "");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.get_by_label("Search").type_text("fix");
    settle(&mut harness);
    commit(&git, "Fix the docs");
    wait_until(&mut harness, "the commit", |h| h.query_by_label("2 of 3").is_some());
}

#[test]
fn create_a_branch_from_the_toolbar() {
    let (_dir, git) = repo();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Branch").click();
    harness.run();
    harness.get_by_label("Name").type_text("feature");
    harness.run();
    harness.get_by_label("Create").click();
    settle(&mut harness);

    assert_eq!(git.run(&["branch", "--show-current"]).unwrap().trim(), "feature");
    harness.get_by_role_and_label(Role::Button, "feature");
}

#[test]
fn a_failing_operation_shows_git_s_message() {
    let (_dir, git) = repo();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Pull").click();
    settle(&mut harness);
    // A reload after a change outside leaves the message to be read.
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    wait_until(&mut harness, "the change", |h| !h.state().workspace().unwrap().snapshot.status.is_clean());
    settle(&mut harness);
    harness.get_by_label("Dismiss").click();
    harness.run();
    assert!(harness.query_by_label("Dismiss").is_none());
    assert_eq!(log(&git), "First commit\n");
}

#[test]
fn open_several_repositories_in_tabs() {
    let (_a_dir, a) = repo();
    let (_b_dir, b) = repo();
    commit(&b, "Only in b");
    let mut harness = app(Some(a.dir()));
    open_in_new_tab(&mut harness, &b);

    assert_eq!(tabs(harness.state()), [name(&a), name(&b)]);
    harness.get_by_role_and_label(Role::Button, "Only in b");
    harness.get_by_label(&name(&a)).click();
    settle(&mut harness);
    assert_eq!(harness.state().active, Some(0));
    assert!(harness.query_by_label("Only in b").is_none());
}

#[test]
fn opening_a_repository_that_is_open_shows_its_tab() {
    let (_a_dir, a) = repo();
    let (_b_dir, b) = repo();
    let mut harness = app(Some(a.dir()));
    open_in_new_tab(&mut harness, &b);
    open_in_new_tab(&mut harness, &a);

    assert_eq!(tabs(harness.state()), [name(&a), name(&b)]);
    assert_eq!(harness.state().active, Some(0));
}

#[test]
fn a_tab_shows_what_changed_while_it_was_out_of_sight() {
    let (_a_dir, a) = repo();
    let (_b_dir, b) = repo();
    let mut harness = app(Some(a.dir()));
    open_in_new_tab(&mut harness, &b);
    commit(&a, "Made elsewhere");
    harness.get_by_label(&name(&a)).click();
    settle(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Made elsewhere");
}

#[test]
fn closing_tabs_shows_the_next_and_then_the_start_screen() {
    let repos = [repo(), repo(), repo()];
    let [a, b, c] = repos.each_ref().map(|(_, git)| git);
    let mut harness = app(Some(a.dir()));
    open_in_new_tab(&mut harness, b);
    open_in_new_tab(&mut harness, c);
    harness.get_by_label(&name(b)).click();
    settle(&mut harness);

    harness.get_by_label(&format!("Close {}", name(b))).click();
    settle(&mut harness);
    assert_eq!(tabs(harness.state()), [name(a), name(c)]);
    assert_eq!(harness.state().active, Some(1));
    harness.get_by_label(&format!("Close {}", name(a))).click();
    settle(&mut harness);
    assert_eq!(harness.state().active, Some(0));
    harness.get_by_label(&format!("Close {}", name(c))).click();
    settle(&mut harness);
    assert!(harness.state().tabs.is_empty());
    harness.get_by_label("Open a repository…");
}

#[test]
fn tabs_open_again_on_the_next_run() {
    let (_a_dir, a) = repo();
    let (_b_dir, b) = repo();
    let (_c_dir, c) = repo();
    let mut harness = app(Some(a.dir()));
    open_in_new_tab(&mut harness, &b);
    harness.get_by_label(&name(&a)).click();
    settle(&mut harness);
    let saved: HashMap<_, _> = KEYS.into_iter().zip(harness.state().saved()).collect();

    let harness = run(App::restore(|key| saved.get(key).cloned(), Some(c.dir().to_owned())));
    assert_eq!(tabs(harness.state()), [name(&a), name(&b), name(&c)]);
    assert_eq!(harness.state().active, Some(2));
    let harness = run(App::restore(|key| saved.get(key).cloned(), None));
    assert_eq!(tabs(harness.state()), [name(&a), name(&b)]);
    assert_eq!(harness.state().active, Some(0));
    let recent: Vec<_> = harness.state().recent.iter().map(|path| fs::canonicalize(path).unwrap()).collect();
    assert_eq!(recent, [a.dir(), b.dir()]);
}

/// Picks `path` from the right-click menu of the widget labelled `on`, one item per step.
fn menu(harness: &mut Harness<'_, App>, on: &str, path: &[&str]) {
    let start = Instant::now();
    // A reload the watcher starts in the frame of the click turns the toolbar off for that click.
    while harness.query_by_label(path[0]).is_none() {
        assert!(start.elapsed() < Duration::from_secs(10), "the menu of {on} never opened");
        harness.get_by_label(on).click_secondary();
        settle(harness);
    }
    for item in path {
        harness.get_by_label(item).click();
        settle(harness);
    }
}

fn rev(git: &Git, rev: &str) -> String {
    git.run(&["rev-parse", rev]).unwrap()
}

#[test]
fn a_hard_reset_asks_first() {
    let (_dir, git) = repo();
    let first = rev(&git, "HEAD");
    commit(&git, "Second commit");
    fs::write(git.dir().join("file.txt"), "unsaved work").unwrap();
    let mut harness = app(Some(git.dir()));
    let reset = ["Reset current branch to this commit", "Hard: discard all changes"];

    menu(&mut harness, "First commit", &reset);
    harness.get_by_label(&format!("Hard reset main to {}, throwing away every uncommitted change?", &first[..7]));
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "unsaved work");

    menu(&mut harness, "First commit", &reset);
    harness.get_by_label("Reset").click();
    settle(&mut harness);
    assert_eq!(log(&git), "First commit\n");
    assert_eq!(read(&git, "file.txt"), "First commit");
}

#[test]
fn deleting_a_branch_asks_only_when_that_loses_commits() {
    let (_dir, git) = repo();
    for name in ["merged", "topic"] {
        git.run(&["branch", name]).unwrap();
    }
    git.apply(&Op::Checkout("topic".into())).unwrap();
    commit(&git, "Work on topic");
    git.apply(&Op::Checkout("main".into())).unwrap();
    let mut harness = app(Some(git.dir()));

    menu(&mut harness, "merged", &["Delete merged"]);
    assert_eq!(git.run(&["branch", "--list", "merged"]).unwrap(), "");
    assert!(harness.query_by_label("Delete").is_none(), "nothing is lost, so nothing is asked");

    menu(&mut harness, "topic", &["Delete topic"]);
    harness.get_by_label("Delete topic and the 1 commit on no other branch or tag?");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_ne!(git.run(&["branch", "--list", "topic"]).unwrap(), "");

    menu(&mut harness, "topic", &["Delete topic"]);
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert_eq!(git.run(&["branch", "--list", "topic"]).unwrap(), "");
    assert!(harness.query_by_label("Work on topic").is_none());
}

#[test]
fn a_branch_that_went_its_own_way_is_force_pushed_once_confirmed() {
    let (_dir, git) = repo();
    let remote = tempfile::tempdir().unwrap();
    Git::new(remote.path()).run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
    git.run(&["remote", "add", "origin", remote.path().to_str().unwrap()]).unwrap();
    git.apply(&Op::Push { remote: Some("origin".into()), track: true, force: false }).unwrap();
    git.apply(&Op::Commit { message: "Amended".into(), amend: true }).unwrap();
    let pushed = || Git::new(remote.path()).run(&["rev-parse", "main"]).unwrap();
    let before = pushed();
    let mut harness = app(Some(git.dir()));

    harness.get_by_label("Push  1").click();
    settle(&mut harness);
    harness.get_by_label("Force push main, throwing away the 1 commit on origin/main that it does not have?");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(pushed(), before);

    harness.get_by_label("Push  1").click();
    settle(&mut harness);
    harness.get_by_label("Force push").click();
    settle(&mut harness);
    assert_eq!(pushed(), rev(&git, "HEAD"));
    assert_eq!(harness.state().workspace().unwrap().error, None);
}

#[test]
fn pull_with_rebase_force_push_to_another_remote_and_push_tags() {
    let (_dir, git) = repo();
    git.run(&["tag", "v1"]).unwrap();
    let dirs = [(); 2].map(|()| tempfile::tempdir().unwrap());
    let [origin, fork] = [0, 1].map(|i| Git::new(dirs[i].path()));
    for (name, remote) in [("origin", &origin), ("fork", &fork)] {
        remote.run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
        git.run(&["remote", "add", name, remote.dir().to_str().unwrap()]).unwrap();
    }
    git.apply(&Op::Push { remote: Some("origin".into()), track: true, force: false }).unwrap();
    // Someone else pushed to origin, and the commit made here meanwhile is on the fork too.
    add_files(&git, &["theirs"]);
    git.apply(&Op::Push { remote: None, track: false, force: false }).unwrap();
    git.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    add_files(&git, &["ours"]);
    git.apply(&Op::Push { remote: Some("fork".into()), track: false, force: false }).unwrap();
    let mut harness = app(Some(git.dir()));

    menu(&mut harness, "Pull  1", &["Pull with rebase"]);
    assert_eq!(log(&git), "Add ours\nAdd theirs\nFirst commit\n");

    // The fork has the commit as it was before the rebase: only a force push replaces it.
    menu(&mut harness, "Push  1", &["Force push to fork…"]);
    harness.get_by_label("Force push main to fork, throwing away what it has there that main does not?");
    harness.get_by_label("Force push").click();
    settle(&mut harness);
    assert_eq!(fork.run(&["rev-parse", "main"]).unwrap(), rev(&git, "HEAD"));

    menu(&mut harness, "Push  1", &["Push tags to fork"]);
    assert_eq!(fork.run(&["tag"]).unwrap(), "v1\n");
    assert_eq!(origin.run(&["tag"]).unwrap(), "");
    let ws = harness.state().workspace().unwrap();
    assert_eq!((ws.snapshot.status.upstream.as_deref(), &ws.error), (Some("origin/main"), &None));
}

#[test]
fn add_edit_and_remove_a_remote() {
    let (_dir, git) = repo();
    let dirs = [(); 2].map(|()| tempfile::tempdir().unwrap());
    let [first, second] = [0, 1].map(|i| dirs[i].path().to_str().unwrap().to_owned());
    for dir in [&first, &second] {
        git.run(&["clone", "-q", "--bare", ".", dir]).unwrap();
    }
    Git::new(&second).run(&["branch", "other", "main"]).unwrap();
    let url = |name: &str| git.run(&["remote", "get-url", name]).map(|url| url.trim().to_owned());
    let mut harness = app(Some(git.dir()));

    // A new remote is fetched from, so that its branches are there at once.
    harness.get_by_label("Add remote…").click();
    harness.run();
    assert!(harness.get_by_label("Name").is_focused(), "ready to type the name");
    type_into(&mut harness, "Name", "origin");
    type_into(&mut harness, "URL", &first);
    harness.get_by_label("Add").click();
    settle(&mut harness);
    assert_eq!(url("origin").unwrap(), first);
    harness.get_by_label("Remote  1");

    menu(&mut harness, "origin", &["Edit remote…"]);
    assert_eq!(harness.get_by_label("URL").value().as_deref(), Some(first.as_str()), "it starts from the URL");
    type_into(&mut harness, "Name", "upstream");
    type_into(&mut harness, "URL", &second);
    harness.get_by_label("Save").click();
    settle(&mut harness);
    assert_eq!(url("upstream").unwrap(), second);
    assert!(url("origin").is_err());
    harness.get_by_label("Fetch").click();
    settle(&mut harness);
    harness.get_by_label("Remote  2");

    menu(&mut harness, "upstream", &["Remove remote…"]);
    harness.get_by_label(&format!("Remove the remote upstream ({second}) and its remote branches?"));
    harness.get_by_label("Remove").click();
    settle(&mut harness);
    assert_eq!(git.run(&["remote"]).unwrap(), "");
    harness.get_by_label("Remote  0");
    assert_eq!(harness.state().workspace().unwrap().error, None);
}

#[test]
fn deleting_a_stash_asks_first() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "stashed").unwrap();
    git.apply(&Op::Stash("wip".into())).unwrap();
    let mut harness = app(Some(git.dir()));

    menu(&mut harness, "On main: wip", &["Delete stash"]);
    harness.get_by_label("Delete the stash \"On main: wip\"?");
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert_eq!(git.run(&["stash", "list"]).unwrap(), "");
}

/// Makes `main` and `topic` each change file.txt their own way, with `main` checked out.
fn clash(git: &Git) {
    git.run(&["branch", "topic"]).unwrap();
    commit(git, "On main");
    git.apply(&Op::Checkout("topic".into())).unwrap();
    commit(git, "On topic");
    git.apply(&Op::Checkout("main".into())).unwrap();
}

#[test]
fn take_their_side_of_a_conflict_and_continue_the_merge() {
    let (_dir, git) = repo();
    clash(&git);
    assert!(git.apply(&Op::Merge("topic".into())).is_err());
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Merge in progress");
    harness.get_by_label("1 conflicted file");
    assert!(harness.get_by_label("Continue").accesskit_node().is_disabled());
    assert!(harness.query_by_label("Skip").is_none(), "a merge has no commit to skip");

    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("Take theirs for file.txt").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "On topic");
    harness.get_by_label("Continue").click();
    settle(&mut harness);
    assert_eq!(git.run(&["log", "-1", "--format=%s"]).unwrap(), "Merge branch 'topic'\n");
    assert!(harness.query_by_label("Merge in progress").is_none());
}

#[test]
fn mark_a_conflict_resolved_and_continue_the_rebase() {
    let (_dir, git) = repo();
    clash(&git);
    git.apply(&Op::Checkout("topic".into())).unwrap();
    assert!(git.apply(&Op::Rebase("main".into())).is_err());
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Rebase in progress");
    harness.get_by_label("Skip");

    fs::write(git.dir().join("file.txt"), "On both").unwrap();
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("Mark resolved").click();
    settle(&mut harness);
    harness.get_by_label("Continue").click();
    settle(&mut harness);
    assert_eq!(log(&git), "On topic\nOn main\nFirst commit\n");
    assert_eq!(read(&git, "file.txt"), "On both");
    assert!(harness.query_by_label("Rebase in progress").is_none());
}

#[test]
fn abort_a_cherry_pick_that_stopped_at_a_conflict() {
    let (_dir, git) = repo();
    clash(&git);
    assert!(git.apply(&Op::CherryPick("topic".into())).is_err());
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Cherry-pick in progress");
    assert_eq!(harness.get_all_by_label("On topic").count(), 2, "the commit in the graph, and above it");

    harness.get_by_label("Abort").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), "On main");
    assert!(harness.query_by_label("Cherry-pick in progress").is_none());
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn resolve_a_file_conflict_by_conflict() {
    let (_dir, git) = repo();
    let lines = |second: &str, last: &str| format!("1\n{second}\n3\n4\n5\n6\n7\n{last}\n");
    let save = |text: String, message: &str| {
        fs::write(git.dir().join("file.txt"), text).unwrap();
        git.apply(&Op::Stage(vec!["file.txt".into()])).unwrap();
        git.apply(&Op::Commit { message: message.into(), amend: false }).unwrap();
    };
    save(lines("2", "8"), "Eight lines");
    git.run(&["branch", "topic"]).unwrap();
    save(lines("main", "main"), "On main");
    git.apply(&Op::Checkout("topic".into())).unwrap();
    save(lines("topic", "topic"), "On topic");
    git.apply(&Op::Checkout("main".into())).unwrap();
    assert!(git.apply(&Op::Merge("topic".into())).is_err());
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);
    harness.get_by_label("Conflict 1 of 2");
    harness.get_by_label("Ours: HEAD");
    harness.get_by_label("Theirs: topic");
    assert!(harness.get_by_label("Save and mark resolved").accesskit_node().is_disabled());

    harness.get_by_label("Take theirs").click();
    settle(&mut harness);
    harness.get_by_label("Conflict 2 of 2");
    harness.get_by_label("Conflict 1").click();
    settle(&mut harness);
    harness.get_by_label("Take ours").click();
    settle(&mut harness);
    harness.get_by_label("Conflict 2 of 2");
    assert!(harness.get_by_label("Save and mark resolved").accesskit_node().is_disabled());
    harness.get_by_label("Take both").click();
    settle(&mut harness);
    harness.get_by_label("Save and mark resolved").click();
    settle(&mut harness);
    assert_eq!(read(&git, "file.txt"), lines("main", "main\ntopic"));
    assert!(harness.state().workspace().unwrap().merge.is_none(), "a resolved file closes");
    harness.get_by_label("Continue").click();
    settle(&mut harness);
    assert_eq!(git.run(&["log", "-1", "--format=%s"]).unwrap(), "Merge branch 'topic'\n");
}

#[test]
fn a_conflicted_file_changed_outside_starts_again() {
    let (_dir, git) = repo();
    clash(&git);
    assert!(git.apply(&Op::Merge("topic".into())).is_err());
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);
    harness.get_by_label("Take ours").click();
    settle(&mut harness);
    let saveable =
        |harness: &Harness<'_, App>| !harness.get_by_label("Save and mark resolved").accesskit_node().is_disabled();
    assert!(saveable(&harness));

    fs::write(git.dir().join("file.txt"), "<<<<<<< HEAD\nEdited\n=======\nOn topic\n>>>>>>> topic\n").unwrap();
    wait_until(&mut harness, "the edited conflict without its pick", |harness| !saveable(harness));
    fs::write(git.dir().join("file.txt"), "On both").unwrap();
    wait_until(&mut harness, "the explanation", |harness| {
        harness.query_by_label_contains("file.txt has no conflict markers").is_some()
    });
    assert!(harness.state().workspace().unwrap().merge.is_none());
}

/// Drags with the mouse from `from` to `to`, going far enough first to tell it from a click.
fn drag(harness: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2) {
    harness.drag_at(from);
    harness.step();
    for at in [from - egui::vec2(0.0, 10.0), to] {
        harness.hover_at(at);
        harness.step();
    }
    harness.drop_at(to);
    settle(harness);
}

/// Commits a file of its own for each name, so that the commits go in any order.
fn add_files(git: &Git, names: &[&str]) {
    for name in names {
        fs::write(git.dir().join(name), name).unwrap();
        git.apply(&Op::Stage(vec![(*name).into()])).unwrap();
        git.apply(&Op::Commit { message: format!("Add {name}"), amend: false }).unwrap();
    }
}

#[test]
fn reorder_squash_and_drop_commits_in_an_interactive_rebase() {
    let (_dir, git) = repo();
    add_files(&git, &["one", "two", "three"]);
    let mut harness = app(Some(git.dir()));
    menu(&mut harness, "First commit", &["Interactive rebase onto this commit…"]);
    harness.get_by_label(&format!("Interactive rebase onto {}", &rev(&git, "HEAD~3")[..7]));
    assert!(harness.get_by_label("Move Add three up").accesskit_node().is_disabled());

    harness.get_by_label("Squash Add one").click();
    settle(&mut harness);
    let start = |harness: &Harness<'_, App>| !harness.get_by_label("Start rebase").accesskit_node().is_disabled();
    assert!(!start(&harness), "the oldest commit has nothing to meld into");
    harness.get_by_label("Drop Add one").click();
    settle(&mut harness);
    harness.get_by_label("Squash Add two").click();
    settle(&mut harness);
    assert!(!start(&harness), "nor has the oldest one it keeps");

    harness.get_by_label("Pick Add two").click();
    harness.get_by_label("Pick Add one").click();
    harness.get_by_label("Move Add one up").click();
    settle(&mut harness);
    // Newest first: three, one, two. Dragging one onto three puts it in its place.
    let from = harness.get_by_label("Add one").rect().center();
    let to = egui::pos2(from.x, harness.get_by_label("Pick Add three").rect().center().y);
    drag(&mut harness, from, to);
    // Newest first: one, three, two.
    harness.get_by_label("Squash Add one").click();
    harness.get_by_label("Drop Add two").click();
    settle(&mut harness);
    assert!(start(&harness));
    harness.get_by_label("Start rebase").click();
    settle(&mut harness);
    assert_eq!(git.run(&["log", "--format=%B|"]).unwrap(), "Add three\n\nAdd one\n|\nFirst commit\n|\n");
    assert_eq!(git.run(&["ls-files"]).unwrap(), "file.txt\none\nthree\n");
    assert!(harness.state().workspace().unwrap().plan.is_none());
    assert_eq!(harness.state().workspace().unwrap().error, None);
}

#[test]
fn reword_a_commit_in_an_interactive_rebase() {
    let (_dir, git) = repo();
    add_files(&git, &["one", "two"]);
    let mut harness = app(Some(git.dir()));
    menu(&mut harness, "First commit", &["Interactive rebase onto this commit…"]);
    harness.get_by_label("Reword Add one").click();
    settle(&mut harness);
    let field = "New message of Add one";
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().workspace().unwrap().plan.is_none(), "Escape leaves the rebase");
    menu(&mut harness, "First commit", &["Interactive rebase onto this commit…"]);
    harness.get_by_label("Reword Add one").click();
    settle(&mut harness);
    assert_eq!(harness.get_by_label(field).value().as_deref(), Some("Add one"), "it starts from the old message");

    type_into(&mut harness, field, " ");
    assert!(harness.get_by_label("Start rebase").accesskit_node().is_disabled(), "a message cannot be empty");
    type_into(&mut harness, field, "One\n\nWith a body");
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().workspace().unwrap().plan.is_some(), "Escape in the message leaves only the message");
    harness.get_by_label("Pick Add one").click();
    settle(&mut harness);
    assert!(harness.query_by_label(field).is_none());
    harness.get_by_label("Reword Add one").click();
    settle(&mut harness);
    assert_eq!(harness.get_by_label(field).value().as_deref(), Some("One\n\nWith a body"), "the new one stays");

    harness.get_by_label("Start rebase").click();
    settle(&mut harness);
    assert_eq!(git.run(&["log", "--format=%B|"]).unwrap(), "Add two\n|\nOne\n\nWith a body\n|\nFirst commit\n|\n");
    assert_eq!(harness.state().workspace().unwrap().error, None);
}

#[test]
fn reword_a_commit_from_its_menu() {
    let (_dir, git) = repo();
    add_files(&git, &["one", "two"]);
    let mut harness = app(Some(git.dir()));
    menu(&mut harness, "First commit", &["Reword commit…"]);
    harness.get_by_label_contains("Only commits of the current branch can be moved or reworded");
    assert!(harness.state().workspace().unwrap().plan.is_none());

    menu(&mut harness, "Add one", &["Reword commit…"]);
    harness.get_by_label(&format!("Interactive rebase onto {}", &rev(&git, "HEAD~2")[..7]));
    let field = harness.get_by_label("New message of Add one");
    assert!(field.is_focused(), "the cursor is in the new message");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("New message of Add one").type_text("One");
    harness.run();
    harness.get_by_label("Start rebase").click();
    settle(&mut harness);
    assert_eq!(log(&git), "Add two\nOne\nFirst commit\n");
    assert_eq!(harness.state().workspace().unwrap().error, None);
}

#[test]
fn drag_a_commit_onto_another_in_the_graph_to_move_it_there() {
    let (_dir, git) = repo();
    git.run(&["checkout", "-q", "-b", "side"]).unwrap();
    add_files(&git, &["side"]);
    git.run(&["checkout", "-q", "main"]).unwrap();
    add_files(&git, &["one", "two", "three"]);
    let mut harness = app(Some(git.dir()));
    let drag_onto = |harness: &mut Harness<'_, App>, from: &str, onto: &str| {
        let [from, onto] =
            [from, onto].map(|summary| harness.get_by_role_and_label(Role::Button, summary).rect().center());
        drag(harness, from, onto);
    };
    drag_onto(&mut harness, "Add three", "Add side");
    harness.get_by_label_contains("Only commits of the current branch can be moved or reworded");
    assert!(harness.state().workspace().unwrap().plan.is_none());

    // Older onto newer and newer onto older: each takes the other's place, in the list to start.
    drag_onto(&mut harness, "Add one", "Add two");
    harness.get_by_label(&format!("Interactive rebase onto {}", &rev(&git, "HEAD~3")[..7]));
    let order = |harness: &Harness<'_, App>| {
        let plan = harness.state().workspace().unwrap().plan.as_ref().unwrap();
        plan.steps.iter().map(|(_, id)| format!("{id}\n")).collect::<Vec<_>>()
    };
    assert_eq!(order(&harness), ["HEAD", "HEAD~2", "HEAD~1"].map(|at| rev(&git, at)));
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    drag_onto(&mut harness, "Add three", "Add one");
    assert_eq!(order(&harness), ["HEAD~1", "HEAD~2", "HEAD"].map(|at| rev(&git, at)));
    assert_eq!(log(&git), "Add three\nAdd two\nAdd one\nFirst commit\n", "nothing happens before the start");
    harness.get_by_label("Start rebase").click();
    settle(&mut harness);
    assert_eq!(harness.state().workspace().unwrap().error, None);
    assert_eq!(log(&git), "Add two\nAdd one\nAdd three\nFirst commit\n");
}

#[test]
fn a_rebase_plan_starts_again_when_its_commits_change_outside() {
    let (_dir, git) = repo();
    add_files(&git, &["one"]);
    let mut harness = app(Some(git.dir()));
    menu(&mut harness, "First commit", &["Interactive rebase onto this commit…"]);
    harness.get_by_label("Drop Add one").click();
    settle(&mut harness);

    add_files(&git, &["two"]);
    wait_until(&mut harness, "the new commit", |harness| harness.query_by_label("Drop Add two").is_some());
    harness.get_by_label("2 commits");
    harness.get_by_label("Start rebase").click();
    settle(&mut harness);
    assert_eq!(log(&git), "Add two\nAdd one\nFirst commit\n", "nothing was dropped");

    menu(&mut harness, "Add two", &["Interactive rebase onto this commit…"]);
    assert!(harness.state().workspace().unwrap().plan.is_none());
    harness.get_by_label_contains("Nothing to rebase onto");
}

/// Whether the history of a file is in the centre and read.
fn history_read(harness: &Harness<'_, App>) -> bool {
    harness.state().workspace().unwrap().history.as_ref().is_some_and(|history| history.reading.is_none())
}

/// Whether the commit `summary` is shown and picked in the history.
fn picked(harness: &Harness<'_, App>, summary: &str) -> bool {
    harness.get_by_role_and_label(Role::Button, summary).accesskit_node().toggled()
        == Some(egui::accesskit::Toggled::True)
}

#[test]
fn follow_a_file_through_its_history_and_a_rename() {
    let (_dir, git) = repo();
    git.run(&["mv", "file.txt", "renamed.txt"]).unwrap();
    git.apply(&Op::Commit { message: "Rename the file".into(), amend: false }).unwrap();
    fs::write(git.dir().join("other.txt"), "other").unwrap();
    git.apply(&Op::Stage(vec!["other.txt".into()])).unwrap();
    git.apply(&Op::Commit { message: "Another file".into(), amend: false }).unwrap();
    fs::write(git.dir().join("renamed.txt"), "Changed since").unwrap();
    let mut harness = app(Some(git.dir()));

    // From the uncommitted changes, it starts at HEAD and shows the newest commit that changed it.
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    menu(&mut harness, "renamed.txt", &["File history"]);
    wait_until(&mut harness, "the history", history_read);
    harness.run();
    harness.get_by_label("History of renamed.txt");
    harness.get_by_label("2 commits");
    assert!(harness.query_by_role_and_label(Role::Button, "Another file").is_none());
    assert!(picked(&harness, "Rename the file"));
    let ws = harness.state().workspace().unwrap();
    let file = ws.open_file.as_ref().expect("a diff is open");
    assert!(file.diff.header.iter().any(|line| line == "rename from file.txt"), "{:?}", file.diff.header);
    harness.get_by_label("Renamed from file.txt, nothing else changed");
    assert_eq!(ws.selection, Some(Selection::Commit(ws.snapshot.history.ids[1])));

    // Before the rename, it was another file.
    harness.get_by_role_and_label(Role::Button, "First commit").click();
    settle(&mut harness);
    assert!(picked(&harness, "First commit"));
    assert_eq!(harness.state().workspace().unwrap().open_file.as_ref().unwrap().source.path(), "file.txt");

    // A commit made outside joins it.
    git.apply(&Op::Stage(vec!["renamed.txt".into()])).unwrap();
    git.apply(&Op::Commit { message: "Change it".into(), amend: false }).unwrap();
    wait_until(&mut harness, "the new commit", |h| h.query_by_label("3 commits").is_some());
    harness.get_by_role_and_label(Role::Button, "Change it");

    harness.get_by_label("Close").click();
    settle(&mut harness);
    let ws = harness.state().workspace().unwrap();
    assert!(ws.history.is_none() && ws.open_file.is_none());

    // From a commit, it starts there.
    harness.get_by_role_and_label(Role::Button, "First commit").click();
    settle(&mut harness);
    menu(&mut harness, "file.txt", &["File history"]);
    wait_until(&mut harness, "the history", history_read);
    harness.run();
    harness.get_by_label("History of file.txt");
    harness.get_by_label("1 commit");
}

/// Whether the blame of a file is in the centre and read.
fn blame_read(harness: &Harness<'_, App>) -> bool {
    harness.state().workspace().unwrap().blame.as_ref().is_some_and(|blame| blame.reading.is_none())
}

#[test]
fn blame_tells_which_commit_last_changed_each_line() {
    let (_dir, git) = repo();
    let change = |text: &str, message: &str| {
        fs::write(git.dir().join("file.txt"), text).unwrap();
        git.apply(&Op::Stage(vec!["file.txt".into()])).unwrap();
        git.apply(&Op::Commit { message: message.into(), amend: false }).unwrap();
    };
    change("one\ntwo\nthree\n", "Count to three");
    git.run(&["config", "user.name", "Otto"]).unwrap();
    change("one\nTWO\nthree\n", "Shout two");
    fs::write(git.dir().join("file.txt"), "one\nTWO\nthree\nfour\n").unwrap();
    let mut harness = app(Some(git.dir()));

    // From the uncommitted changes, it is the file in the working tree.
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    menu(&mut harness, "file.txt", &["Blame"]);
    wait_until(&mut harness, "the blame", blame_read);
    harness.run();
    for label in ["Blame of file.txt", "in the working tree", "4 lines", "Line 1: one (Count to three)"] {
        harness.get_by_label(label);
    }
    harness.get_by_label("Line 3: three (Count to three)");
    harness.get_by_label("Line 4: four (Not committed yet)");

    // A line shows the commit that changed it, and the other lines it changed.
    harness.get_by_label("Line 2: TWO (Shout two)").click();
    settle(&mut harness);
    assert!(picked(&harness, "Line 2: TWO (Shout two)"));
    assert!(!picked(&harness, "Line 1: one (Count to three)"));
    harness.get_by_label("Otto <tess@example.com>");
    let ws = harness.state().workspace().unwrap();
    assert_eq!(ws.selection, Some(Selection::Commit(ws.snapshot.history.ids[0])));

    // What changes outside shows up, before and after it is committed.
    fs::write(git.dir().join("file.txt"), "one\nTWO\nthree\nfour\nfive\n").unwrap();
    wait_until(&mut harness, "the new line", |h| h.query_by_label("Line 5: five (Not committed yet)").is_some());
    change("one\nTWO\nthree\nfour\nfive\n", "Count on");
    wait_until(&mut harness, "the new commit", |h| h.query_by_label("Line 5: five (Count on)").is_some());
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().workspace().unwrap().blame.is_none());

    // From a commit, it is the file in that commit.
    harness.get_by_role_and_label(Role::Button, "Count to three").click();
    settle(&mut harness);
    menu(&mut harness, "file.txt", &["Blame"]);
    wait_until(&mut harness, "the blame", blame_read);
    harness.run();
    harness.get_by_label(&format!("at {}", &rev(&git, "HEAD~2")[..7]));
    harness.get_by_label("Line 2: two (Count to three)");
    harness.get_by_label("Close").click();
    settle(&mut harness);

    // A file a commit deleted has a history, but no lines to blame.
    git.apply(&Op::Remove(vec!["file.txt".into()])).unwrap();
    git.apply(&Op::Commit { message: "Delete the file".into(), amend: false }).unwrap();
    wait_until(&mut harness, "the commit", |h| h.query_by_role_and_label(Role::Button, "Delete the file").is_some());
    harness.get_by_role_and_label(Role::Button, "Delete the file").click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click_secondary();
    wait_until(&mut harness, "the menu", |h| h.query_by_label("File history").is_some());
    assert!(harness.query_by_label("Blame").is_none());
}

#[test]
fn the_changed_files_stay_in_reach_below_a_long_commit_message() {
    let (_dir, git) = repo();
    let body: Vec<_> = (1..=100).map(|n| format!("Line {n} of a long story")).collect();
    commit(&git, &format!("A long story\n\n{}", body.join("\n")));
    let harness = app(Some(git.dir()));
    harness.get_by_label_contains("Line 100 of a long story");
    let file = harness.get_by_label("file.txt").rect();
    assert!(file.bottom() <= 720.0, "the changed file is at {file:?}, below the window");
}

/// Opens the command palette and types `words` in it.
fn palette(harness: &mut Harness<'_, App>, words: &str) {
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
    settle(harness);
    assert!(harness.get_by_label("Command").is_focused(), "the palette has the keyboard");
    harness.get_by_label("Command").type_text(words);
    settle(harness);
}

/// The names of the commands the palette lists, top to bottom.
fn listed(harness: &Harness<'_, App>) -> Vec<String> {
    harness.query_all_by_role(Role::ListBoxOption).filter_map(|row| row.accesskit_node().label()).collect()
}

#[test]
fn the_command_palette_finds_a_command_by_words_of_its_name() {
    let (_dir, git) = repo();
    let mut harness = app(Some(git.dir()));

    palette(&mut harness, "new br");
    assert_eq!(listed(&harness), ["New branch…"]);
    harness.key_press(Key::Enter);
    settle(&mut harness);
    assert!(harness.state().palette.is_none());
    harness.get_by_label("Name").type_text("feature");
    harness.key_press(Key::Enter);
    settle(&mut harness);
    assert_eq!(git.run(&["branch", "--show-current"]).unwrap().trim(), "feature");

    palette(&mut harness, "no such thing");
    assert_eq!(listed(&harness), Vec::<String>::new());
    harness.get_by_label("No command has those words");
    harness.key_press(Key::Enter);
    settle(&mut harness);
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().palette.is_none());
    // Nothing behind the palette took the keys.
    assert_eq!(harness.state().workspace().unwrap().prompt, None);
}

#[test]
fn the_command_palette_has_what_the_selected_commit_offers() {
    let (_dir, git) = repo();
    let first = rev(&git, "HEAD");
    git.run(&["tag", "v1"]).unwrap();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));
    harness.get_by_role_and_label(Role::Button, "First commit").click();
    settle(&mut harness);
    let commit = format!("Commit {}", &first[..7]);

    palette(&mut harness, "reset");
    let reset = format!("{commit} › Reset current branch to this commit › ");
    let modes = ["Soft: keep all changes", "Mixed: keep working copy", "Hard: discard all changes"];
    assert_eq!(listed(&harness), modes.map(|mode| format!("{reset}{mode}")));
    // Down and up go round at the ends, and the keyboard stays in the field.
    for key in [Key::ArrowDown, Key::ArrowDown, Key::ArrowDown, Key::ArrowUp] {
        harness.key_press(key);
        harness.step();
        assert!(harness.get_by_label("Command").is_focused());
    }
    settle(&mut harness);
    harness.key_press(Key::Enter);
    settle(&mut harness);
    harness.get_by_label(&format!("Hard reset main to {}, throwing away every uncommitted change?", &first[..7]));
    harness.get_by_label("Cancel").click();
    settle(&mut harness);

    palette(&mut harness, "tag");
    assert_eq!(listed(&harness), [format!("{commit} › Delete tag v1"), format!("{commit} › Create tag here…")]);
    harness.key_press(Key::Escape);
    settle(&mut harness);
    palette(&mut harness, "copy sha");
    harness.get_by_label(&format!("{commit} › Copy commit SHA")).click();
    harness.step();
    let copied = &harness.output().platform_output.commands;
    assert!(copied.contains(&egui::OutputCommand::CopyText(first.trim().to_owned())), "{copied:?}");
    settle(&mut harness);
    // The palette took its keyboard along.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut harness);
    assert_eq!(log(&git), "First commit\n");
}

#[test]
fn shortcuts_run_commands_but_leave_a_text_field_its_keys() {
    let (_dir, git) = repo();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut harness);
    assert_eq!(log(&git), "First commit\n");
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    settle(&mut harness);
    assert_eq!(log(&git), "Second commit\nFirst commit\n");

    fs::write(git.dir().join("file.txt"), "Third commit").unwrap();
    git.apply(&Op::Stage(vec!["file.txt".into()])).unwrap();
    wait_until(&mut harness, "the change", |h| h.query_by_label("Uncommitted changes").is_some());
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    harness.get_by_label("Summary").click();
    settle(&mut harness);
    harness.get_by_label("Summary").type_text("Third commit");
    // While typing, a new tab waits; committing does not.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::T);
    settle(&mut harness);
    assert_eq!(harness.state().active, Some(0));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    settle(&mut harness);
    assert_eq!(log(&git), "Third commit\nSecond commit\nFirst commit\n");

    harness.key_press(Key::Escape);
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::T);
    settle(&mut harness);
    assert_eq!(harness.state().active, None);
    // The start screen is the tab after the last.
    harness.key_press_modifiers(Modifiers::CTRL, Key::Tab);
    settle(&mut harness);
    assert_eq!(harness.state().active, Some(0));
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab);
    settle(&mut harness);
    assert_eq!(harness.state().active, None);
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab);
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::W);
    settle(&mut harness);
    assert!(harness.state().tabs.is_empty());
}

#[test]
fn while_git_is_busy_the_keyboard_queues_what_it_runs_apart_from_undo() {
    let (_dir, git) = repo();
    commit(&git, "Second commit");
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    let mut harness = app(Some(git.dir()));
    let ctx = harness.ctx.clone();
    harness.state_mut().workspace_mut().unwrap().apply(&ctx, Action::Refresh);

    let can = |name: &str| {
        let commands = commands::all(&harness.ctx, harness.state());
        commands.into_iter().find(|command| command.name.starts_with(name)).unwrap().action.is_some()
    };
    assert!(can("Stash") && can("Fetch") && can("New branch"));
    assert!(!can("Undo"), "undo waits until it knows where HEAD is");
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    settle(&mut harness);
    assert_eq!(git.run(&["stash", "list", "--format=%s"]).unwrap(), "On main: WIP on main\n");
    assert_eq!(read(&git, "file.txt"), "Second commit");
}

/// The diff in view, if any.
fn in_view(harness: &Harness<'_, App>) -> Option<Source> {
    harness.state().workspace().unwrap().open_file.as_ref().map(|file| file.source.clone())
}

/// Presses `key` with `modifiers`, and lets the app carry out what it does.
fn press(harness: &mut Harness<'_, App>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    settle(harness);
}

/// Commits `files`, each with its text.
fn commit_files(git: &Git, files: &[(&str, &str)], message: &str) {
    for (path, text) in files {
        fs::write(git.dir().join(path), text).unwrap();
    }
    git.apply(&Op::Stage(files.iter().map(|(path, _)| (*path).into()).collect())).unwrap();
    git.apply(&Op::Commit { message: message.into(), amend: false }).unwrap();
}

#[test]
fn stage_unstage_and_discard_the_file_in_view_by_keys() {
    let (_dir, git) = repo();
    commit_files(&git, &[("a.txt", "a\n"), ("b.txt", "one\n")], "Add two files");
    fs::write(git.dir().join("a.txt"), "changed\n").unwrap();
    fs::write(git.dir().join("b.txt"), "one\ntwo\nthree\n").unwrap();
    fs::write(git.dir().join("new.txt"), "new\n").unwrap();
    let staged = || git.run(&["diff", "--cached", "--name-only"]).unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    let cmd = Modifiers::COMMAND;

    // Down and up go from file to file, in the order of the panel.
    press(&mut harness, cmd, Key::ArrowDown);
    assert_eq!(in_view(&harness), Some(Source::Unstaged("a.txt".into())));
    press(&mut harness, cmd, Key::ArrowDown);
    press(&mut harness, cmd, Key::ArrowDown);
    assert_eq!(in_view(&harness), Some(Source::Untracked("new.txt".into())));
    press(&mut harness, cmd, Key::ArrowUp);
    press(&mut harness, cmd, Key::ArrowUp);

    // A file staged as a whole gives its place to the next.
    press(&mut harness, cmd, Key::S);
    assert_eq!(staged(), "a.txt\n");
    assert_eq!(in_view(&harness), Some(Source::Unstaged("b.txt".into())));

    // With lines picked, the keys are for those lines.
    let [two] = painted_at(&harness, "+ two")[..] else { panic!("no line two") };
    click_at(&mut harness, two + egui::vec2(8.0, 4.0));
    press(&mut harness, cmd, Key::S);
    assert_eq!(git.run(&["show", ":b.txt"]).unwrap(), "one\ntwo\n");
    assert_eq!(in_view(&harness), Some(Source::Unstaged("b.txt".into())), "b.txt has a change left");

    // Discarding asks first.
    press(&mut harness, cmd, Key::Backspace);
    harness.get_by_label("Discard the unstaged changes to b.txt?");
    harness.get_by_label("Discard").click();
    settle(&mut harness);
    assert_eq!(read(&git, "b.txt"), "one\ntwo\n");
    assert_eq!(in_view(&harness), Some(Source::Untracked("new.txt".into())));
    press(&mut harness, cmd, Key::S);
    assert_eq!(staged(), "a.txt\nb.txt\nnew.txt\n");
    assert_eq!(in_view(&harness), None, "no unstaged file is left to show");

    // Unstaging goes the same way through the staged files.
    press(&mut harness, cmd, Key::ArrowDown);
    press(&mut harness, cmd, Key::ArrowDown);
    assert_eq!(in_view(&harness), Some(Source::Staged("b.txt".into())));
    press(&mut harness, cmd, Key::U);
    assert_eq!(staged(), "a.txt\nnew.txt\n");
    assert_eq!(in_view(&harness), Some(Source::Staged("new.txt".into())));
}

#[test]
fn the_keyboard_goes_through_a_commit_s_files_and_back_to_the_graph() {
    let (_dir, git) = repo();
    commit_files(&git, &[("a.txt", "a\n"), ("b.txt", "b\n")], "Add a and b");
    let head = rev(&git, "HEAD").trim().to_owned();
    let file = |path: &str| Some(Source::Commit { commit: head.clone(), path: path.into(), from: None });
    let mut harness = app(Some(git.dir()));

    // Down from the graph opens the first file of the selected commit; the last one stays.
    for (key, path) in [(Key::ArrowDown, "a.txt"), (Key::ArrowDown, "b.txt"), (Key::ArrowDown, "b.txt")] {
        press(&mut harness, Modifiers::COMMAND, key);
        assert_eq!(in_view(&harness), file(path));
    }
    press(&mut harness, Modifiers::COMMAND, Key::ArrowUp);
    assert_eq!(in_view(&harness), file("a.txt"));

    // A commit's file has nothing to stage, and the first has none before it.
    palette(&mut harness, "file");
    assert_eq!(listed(&harness), ["Next file", "Previous file"]);
    assert!(harness.get_by_label("Previous file").accesskit_node().is_disabled());
    harness.key_press(Key::Escape);
    settle(&mut harness);
    palette(&mut harness, "back");
    harness.key_press(Key::Enter);
    settle(&mut harness);
    assert_eq!(in_view(&harness), None);
    harness.get_by_role_and_label(Role::Button, "Add a and b");

    // The start screen's dialogs are in the palette too.
    for (words, title) in [("clone", "Clone a repository"), ("create", "Create a repository")] {
        palette(&mut harness, words);
        harness.key_press(Key::Enter);
        settle(&mut harness);
        assert_eq!(harness.state().active, None);
        harness.get_by_label(title);
        harness.get_by_label("Cancel").click();
        settle(&mut harness);
        harness.key_press_modifiers(Modifiers::CTRL, Key::Tab);
        settle(&mut harness);
    }
}

/// How many texts on screen read just `text`.
fn painted(harness: &Harness<'_, App>, text: &str) -> usize {
    let shapes = harness.output().shapes.iter();
    shapes.filter(|clipped| matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.job.text == text)).count()
}

#[test]
fn the_settings_change_how_dates_read_what_pull_does_and_the_zoom() {
    let (_dir, git) = repo();
    let dir = tempfile::tempdir().unwrap();
    let origin = Git::new(dir.path());
    origin.run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
    git.run(&["remote", "add", "origin", origin.dir().to_str().unwrap()]).unwrap();
    git.apply(&Op::Push { remote: Some("origin".into()), track: true, force: false }).unwrap();
    // Someone else pushed to origin while a commit was made here.
    add_files(&git, &["theirs"]);
    git.apply(&Op::Push { remote: None, track: false, force: false }).unwrap();
    git.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    add_files(&git, &["ours"]);
    let date = git.run(&["log", "-1", "--format=%ad", "--date=format:%Y-%m-%d %H:%M"]).unwrap();
    let mut harness = app(Some(git.dir()));
    assert_eq!((painted(&harness, "just now") > 0, painted(&harness, date.trim())), (true, 0));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    settle(&mut harness);
    harness.get_by_label("Date and time").click();
    harness.get_by_label("Rebase").click();
    settle(&mut harness);
    for (button, zoom) in [("Zoom in", 1.1), ("Zoom in", 1.2), ("Zoom out", 1.1), ("Reset the zoom", 1.0)] {
        harness.get_by_label(button).click();
        settle(&mut harness);
        assert!((harness.ctx.zoom_factor() - zoom).abs() < 0.01, "{button} gave {}", harness.ctx.zoom_factor());
    }
    harness.get_by_label("Close").click();
    settle(&mut harness);
    assert!(!harness.state().settings);
    assert_eq!((painted(&harness, "just now"), painted(&harness, date.trim()) > 0), (0, true));

    // Pull rebases now, and its menu still merges or rebases as asked.
    harness.get_by_label("Pull  1").click();
    settle(&mut harness);
    assert_eq!(log(&git), "Add ours\nAdd theirs\nFirst commit\n");

    // The settings are in the tab bar too; Escape closes them, and only them.
    harness.get_by_label("Settings").click();
    settle(&mut harness);
    harness.get_by_label("Merge").click();
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(!harness.state().settings);
    let pull = commands::all(&harness.ctx, harness.state()).into_iter().find(|c| c.name == "Pull").unwrap();
    assert_eq!(pull.action, Some(Action::Run(Op::Pull { rebase: false })));
}

#[test]
fn a_light_theme_colours_diffs_and_code_for_a_light_background() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("main.rs"), "fn main() {}\n").unwrap();
    git.apply(&Op::Stage(vec!["main.rs".into()])).unwrap();
    git.apply(&Op::Commit { message: "Main".into(), amend: false }).unwrap();
    fs::write(git.dir().join("main.rs"), "fn main() { greet(); }\n").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Uncommitted changes").click();
    settle(&mut harness);
    open_versions(&mut harness, "main.rs");
    let dark = coloured(&harness, "fn");
    assert!(painted_on(&harness, theme::DARK.added_word).concat().contains("greet"));

    // In the light theme the diff and its code take the colours for a light background.
    harness.get_by_label("Settings").click();
    settle(&mut harness);
    harness.get_by_label("Light").click();
    harness.key_press(Key::Escape);
    let coloured_again = |h: &Harness<'_, App>| {
        let file = h.state().workspace().unwrap().open_file.as_ref().unwrap();
        file.reading.is_none() && matches!(file.versions, Versions::Code(egui::Theme::Light, _))
    };
    wait_until(&mut harness, "the code in light colours", coloured_again);
    settle(&mut harness);
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
    let light = coloured(&harness, "fn");
    assert!(dark.len() == 2 && light.len() == 2 && light[0] != dark[0], "{dark:?} and {light:?}");
    assert!(painted_on(&harness, theme::LIGHT.added_word).concat().contains("greet"));
    assert_eq!(painted_on(&harness, theme::DARK.added_word), Vec::<String>::new());

    // Or the one the system uses.
    harness.get_by_label("Settings").click();
    settle(&mut harness);
    harness.get_by_label("System").click();
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert_eq!(harness.ctx.options(|o| o.theme_preference), egui::ThemePreference::System);
}

/// Double-clicks with the mouse at `at`, both clicks in one frame.
fn double_click_at(harness: &mut Harness<'_, App>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.step();
    for pressed in [true, false, true, false] {
        let (button, modifiers) = (egui::PointerButton::Primary, Modifiers::NONE);
        harness.input_mut().events.push(egui::Event::PointerButton { pos: at, button, pressed, modifiers });
    }
    harness.step();
    settle(harness);
}

#[test]
fn drag_the_borders_between_the_names_of_the_columns_to_make_them_wider_or_narrower() {
    let (_dir, git) = repo();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));
    harness.set_size(egui::vec2(1800.0, 720.0));
    settle(&mut harness);
    let row = harness.get_by_role_and_label(Role::Button, "First commit").rect();
    // Where the message and the author of the first commit start in its row.
    let starts = |harness: &Harness<'_, App>| {
        let start =
            |text| painted_at(harness, text).into_iter().find(|at| row.contains(*at)).map(|at| at.x - row.left());
        (start("First commit").unwrap(), start("Tess Ter"))
    };
    let border = |harness: &Harness<'_, App>, column: &str| {
        harness.get_by_label(&format!("{column} column width")).rect().center()
    };
    let drag_by = |harness: &mut Harness<'_, App>, column: &str, x: f32| {
        let from = border(harness, column);
        drag(harness, from, from + egui::vec2(x, 0.0));
    };
    let (message, author) = starts(&harness);
    let author = author.expect("room for the author");
    assert_eq!(["Branches and tags", "Graph", "Message", "Author", "Date"].map(|name| painted(&harness, name)), [1; 5]);

    // The border on the right of the branches and the graph, and on the left of the author.
    drag_by(&mut harness, "Graph", 60.0);
    let (moved, same_author) = starts(&harness);
    assert!(same(moved, message + 60.0) && same(same_author.unwrap(), author), "{moved} and {same_author:?}");
    drag_by(&mut harness, "Branches and tags", -50.0);
    assert!(same(starts(&harness).0, message + 10.0));
    drag_by(&mut harness, "Author", -30.0);
    let (unmoved, wider) = starts(&harness);
    assert!(same(unmoved, message + 10.0) && same(wider.unwrap(), author - 30.0), "{unmoved} and {wider:?}");

    // A double click gives a column the width it starts with.
    let graph = border(&harness, "Graph");
    double_click_at(&mut harness, graph);
    assert!(same(starts(&harness).0, message - 50.0), "{:?}", starts(&harness));

    // The branches and the graph get a third of the width at most, and the author goes before the
    // message gets too narrow.
    drag_by(&mut harness, "Branches and tags", 2_000.0);
    let lanes = message - 8.0 - 170.0;
    assert!(same(starts(&harness).0, row.width() / 3.0 + lanes + 8.0), "{:?} in {}", starts(&harness), row.width());
    harness.set_size(egui::vec2(1280.0, 720.0));
    settle(&mut harness);
    assert_eq!(starts(&harness).1, None);
    assert!(harness.query_by_label("Author column width").is_none());
    assert_eq!(painted(&harness, "Author"), 0);
}
