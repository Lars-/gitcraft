//! The whole app, driven through its accessibility tree against real repositories.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{Key, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use gitcraft_core::git::{Git, Op};
use tempfile::TempDir;

use crate::app::Selection;
use crate::start::percent;
use crate::ui::App;

/// A repository with one commit, with the tester's own git config kept out of the way.
fn repo() -> (TempDir, Git) {
    let dir = tempfile::tempdir().unwrap();
    let git = Git::new(fs::canonicalize(dir.path()).unwrap());
    git.run(&["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "Tess Ter"), ("user.email", "tess@example.com"), ("commit.gpgsign", "false")] {
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
    let app = App::new(Vec::new(), path.map(Path::to_path_buf));
    let mut harness =
        Harness::builder().with_size([1280.0, 720.0]).build_ui_state(|ui, app: &mut App| app.show(ui), app);
    settle(&mut harness);
    harness
}

/// Steps the app until its background job is done and the frame is stable.
fn settle(harness: &mut Harness<'_, App>) {
    let start = Instant::now();
    harness.step();
    while busy(harness.state()) {
        assert!(start.elapsed() < Duration::from_secs(30), "the app stays busy");
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
    harness.run();
}

fn busy(app: &App) -> bool {
    match &app.workspace {
        Some(ws) => ws.busy().is_some(),
        None => app.start.dialog.as_ref().is_some_and(crate::start::Dialog::busy),
    }
}

/// Types into the text field labelled `label`.
fn type_into(harness: &mut Harness<'_, App>, label: &str, text: &str) {
    harness.get_by_label(label).click();
    harness.run();
    harness.get_by_label(label).type_text(text);
    harness.run();
}

#[test]
fn the_start_screen_offers_to_open_a_repository() {
    let harness = app(None);
    harness.get_by_label("Open a repository…");
}

#[test]
fn a_bad_path_explains_itself_on_the_start_screen() {
    let harness = app(Some(Path::new("/surely/not/a/repository")));
    assert!(harness.state().workspace.is_none());
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

    let ws = harness.state().workspace.as_ref().expect("the clone is open");
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
    type_into(&mut harness, "Folder", into.path().to_str().unwrap());
    harness.get_by_label("Clone").click();
    settle(&mut harness);

    assert!(harness.state().workspace.is_none());
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

    assert_eq!(harness.state().workspace.as_ref().expect("the new repository is open").repo.name(), "project");
    assert!(into.path().join("project/.git").is_dir());
}

#[test]
fn git_s_progress_lines_fill_the_progress_bar() {
    assert_eq!(percent("Receiving objects:  50% (500/1000), 1.20 MiB | 2.00 MiB/s"), Some(0.5));
    assert_eq!(percent("remote: Counting objects: 100% (9/9), done."), Some(1.0));
    assert_eq!(percent("Cloning into 'repo'..."), None);
}

#[test]
fn stage_and_commit_a_change() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_role_and_label(Role::Button, "First commit");

    harness.get_by_label("Uncommitted changes").click();
    harness.run();
    harness.get_by_label("Stage").click();
    settle(&mut harness);
    harness.state_mut().workspace.as_mut().unwrap().summary = "Second commit".into();
    harness.run();
    harness.get_by_label("Commit changes to 1 file").click();
    settle(&mut harness);

    assert_eq!(log(&git), "Second commit\nFirst commit\n");
    harness.get_by_role_and_label(Role::Button, "Second commit");
    assert!(harness.query_by_label("Uncommitted changes").is_none());
}

#[test]
fn arrow_keys_walk_the_history() {
    let (_dir, git) = repo();
    commit(&git, "Second commit");
    let mut harness = app(Some(git.dir()));
    let ids = harness.state().workspace.as_ref().unwrap().snapshot.history.ids.clone();
    assert_eq!(harness.state().workspace.as_ref().unwrap().selection, Some(Selection::Commit(ids[0])));

    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(harness.state().workspace.as_ref().unwrap().selection, Some(Selection::Commit(ids[1])));
    harness.key_press(Key::ArrowUp);
    harness.run();
    assert_eq!(harness.state().workspace.as_ref().unwrap().selection, Some(Selection::Commit(ids[0])));
}

#[test]
fn create_a_branch_from_the_toolbar() {
    let (_dir, git) = repo();
    let mut harness = app(Some(git.dir()));
    harness.get_by_label("Branch").click();
    harness.run();
    harness.get_by_role(Role::TextInput).type_text("feature");
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
    harness.get_by_label("Dismiss");
    harness.get_by_label("Dismiss").click();
    harness.run();
    assert!(harness.query_by_label("Dismiss").is_none());
    assert_eq!(log(&git), "First commit\n");
}
