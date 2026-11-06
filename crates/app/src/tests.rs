//! The whole app, driven through its accessibility tree against real repositories.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{Key, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use gitcraft_core::git::{Git, Op};
use tempfile::TempDir;

use crate::app::{Action, Selection};
use crate::start::percent;
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

/// Picks `path` from the right-click menu of the widget labelled `on`, one item per step.
fn menu(harness: &mut Harness<'_, App>, on: &str, path: &[&str]) {
    harness.get_by_label(on).click_secondary();
    settle(harness);
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
    git.apply(&Op::Push { upstream: Some("origin".into()), force: false }).unwrap();
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
    harness.get_by_label("Move Add one up").click();
    settle(&mut harness);
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
