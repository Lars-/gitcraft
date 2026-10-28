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
    while harness.state().workspace.as_ref().is_some_and(|ws| ws.busy().is_some()) {
        assert!(start.elapsed() < Duration::from_secs(30), "the app stays busy");
        std::thread::sleep(Duration::from_millis(5));
        harness.step();
    }
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
