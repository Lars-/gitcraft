//! The whole app, driven through its accessibility tree against real repositories.

use std::collections::HashMap;
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
use crate::ui::{App, KEYS};

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
    run(App::restore(|_| None, path.map(Path::to_path_buf)))
}

fn run(app: App) -> Harness<'static, App> {
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
fn stage_and_commit_a_change() {
    let (_dir, git) = repo();
    fs::write(git.dir().join("file.txt"), "changed").unwrap();
    let mut harness = app(Some(git.dir()));
    harness.get_by_role_and_label(Role::Button, "First commit");

    harness.get_by_label("Uncommitted changes").click();
    harness.run();
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
