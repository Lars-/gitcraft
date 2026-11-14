//! End-to-end tests against real repositories made with the installed `git`.

use std::collections::BTreeMap;
use std::fs;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use gitcraft_core::askpass;
use gitcraft_core::conflict::{Conflicts, Pick};
use gitcraft_core::diff::{self, FileDiff, LineKind, Source};
use gitcraft_core::git::{self, Git, Op, Remote, ResetMode, Step};
use gitcraft_core::repo::{Label, RefKind, Repo, Underway};
use gitcraft_core::status::{self, Change, Status};
use gitcraft_core::undo::{self, Move, Undo};
use gitcraft_core::watch::Watch;
use gitcraft_core::{Error, ObjectId};
use tempfile::TempDir;

/// A new repository with settings that keep the tester's own git config out of the way.
fn repo() -> (TempDir, Git) {
    let dir = tempfile::tempdir().unwrap();
    let git = init(Git::new(fs::canonicalize(dir.path()).unwrap()), &["init", "-q", "-b", "main"]);
    (dir, git)
}

fn init(git: Git, command: &[&str]) -> Git {
    git.run(command).unwrap();
    let hooks = git.dir().join(".git/no-hooks");
    for (key, value) in [
        ("user.name", "Tess Ter"),
        ("user.email", "tess@example.com"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("merge.ff", "true"),
        ("core.hooksPath", hooks.to_str().unwrap()),
    ] {
        git.run(&["config", key, value]).unwrap();
    }
    git
}

fn write(git: &Git, path: &str, content: &str) {
    fs::write(git.dir().join(path), content).unwrap();
}

fn commit(git: &Git, path: &str, content: &str, message: &str) -> String {
    write(git, path, content);
    git.apply(&Op::Stage(vec![path.into()])).unwrap();
    git.apply(&Op::Commit { message: message.into(), amend: false }).unwrap();
    head(git)
}

fn head(git: &Git) -> String {
    git.run(&["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

fn status(git: &Git) -> Vec<(String, Option<Change>, Option<Change>)> {
    Status::read(git).unwrap().entries.into_iter().map(|e| (e.path, e.staged, e.unstaged)).collect()
}

fn id(sha: &str) -> ObjectId {
    ObjectId::from_hex(sha.as_bytes()).unwrap()
}

#[test]
fn history_has_every_branch_and_tag_in_graph_order() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "a\n", "Root");
    commit(&git, "a.txt", "a\nb\n", "Main work");
    git.apply(&Op::Branch { name: "feature".into(), at: root.clone() }).unwrap();
    let feature = commit(&git, "f.txt", "f\n", "Feature work");
    git.apply(&Op::Checkout("main".into())).unwrap();
    git.apply(&Op::Merge("feature".into())).unwrap();
    git.apply(&Op::Tag { name: "v1".into(), at: root.clone() }).unwrap();

    let repo = Repo::open(git.dir().join(".git")).unwrap();
    let history = repo.history().unwrap();
    assert_eq!(history.ids.len(), 4);
    assert_eq!(history.head, Some(0));
    for row in 0..history.graph.len() {
        assert!(history.graph.parents(row).all(|parent| parent > row));
    }
    assert_eq!(history.graph.parents(0).len(), 2);
    let label = |name: &str, kind| Label { name: name.into(), kind };
    assert_eq!(history.labels[&0], [label("main", RefKind::Local)]);
    assert_eq!(history.labels[&history.row(id(&feature)).unwrap()], [label("feature", RefKind::Local)]);
    assert_eq!(history.labels[&history.row(id(&root)).unwrap()], [label("v1", RefKind::Tag)]);
    assert_eq!(history.graph.width(), 2);

    let merge = repo.commit(history.ids[0]).unwrap();
    assert_eq!(merge.summary(), "Merge branch 'feature'");
    assert_eq!((merge.author.as_str(), merge.email.as_str()), ("Tess Ter", "tess@example.com"));
    assert!(merge.time.seconds > 1_700_000_000);

    let files = status::commit_files(&git, &history.ids[0].to_string()).unwrap();
    assert_eq!(files.iter().map(|f| (f.path.as_str(), f.change)).collect::<Vec<_>>(), [("f.txt", Change::Added)]);
}

#[test]
fn search_finds_messages_authors_shas_branches_and_tags_in_either_case() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "a\n", "Root");
    let message = "Fix the parser\n\nÜber-long lines no more";
    git.run(&["commit", "-q", "--allow-empty", "--author", "Ada Lovelace <ada@example.org>", "-m", message]).unwrap();
    git.apply(&Op::Tag { name: "Shipped".into(), at: "HEAD".into() }).unwrap();
    commit(&git, "a.txt", "b\n", "Fix the printer");

    // Top to bottom: the printer fix on main, the parser fix by Ada with the tag, the root.
    let repo = Repo::open(git.dir()).unwrap();
    let history = repo.history().unwrap();
    let search = |text: &str| repo.search(&history, 0..3, text).unwrap();
    assert_eq!(search("fix"), [0, 1]);
    assert_eq!(search("üBER-LONG"), [1]);
    assert_eq!((search("lovelace"), search("EXAMPLE.ORG"), search("tess@")), (vec![1], vec![1], vec![0, 2]));
    assert_eq!((search("MAIN"), search("shipped")), (vec![0], vec![1]));
    assert_eq!(search(&root[..4].to_uppercase()), [2]);
    // Fewer than four hex digits, or digits from the middle, are no SHA; the root has no such text.
    assert!(!search(&root[..3]).contains(&2) && !search(&root[1..8]).contains(&2));
    assert_eq!((search("no such text"), repo.search(&history, 1..3, "fix").unwrap()), (vec![], vec![1]));
}

#[test]
fn opens_from_a_subfolder_and_handles_an_empty_repository() {
    let (_dir, git) = repo();
    fs::create_dir(git.dir().join("sub")).unwrap();
    let repo = Repo::open(git.dir().join("sub")).unwrap();
    assert_eq!(fs::canonicalize(repo.git.dir()).unwrap(), git.dir());
    assert_eq!(repo.name(), git.dir().file_name().unwrap().to_string_lossy());
    let history = repo.history().unwrap();
    assert!(history.ids.is_empty() && history.head.is_none());
    assert!(Repo::open(std::env::temp_dir().join("gitcraft-surely-missing")).is_err());
}

#[test]
fn stage_unstage_discard_and_delete() {
    let (_dir, git) = repo();
    commit(&git, "a.txt", "a\n", "Root");
    write(&git, "a.txt", "changed\n");
    write(&git, "new.txt", "new\n");
    assert_eq!(
        status(&git),
        [("a.txt".into(), None, Some(Change::Modified)), ("new.txt".into(), None, Some(Change::Untracked))]
    );

    git.apply(&Op::Stage(vec!["a.txt".into(), "new.txt".into()])).unwrap();
    assert_eq!(
        status(&git),
        [("a.txt".into(), Some(Change::Modified), None), ("new.txt".into(), Some(Change::Added), None)]
    );

    git.apply(&Op::Unstage(vec!["a.txt".into(), "new.txt".into()])).unwrap();
    git.apply(&Op::Discard(vec!["a.txt".into()])).unwrap();
    git.apply(&Op::DeleteUntracked(vec!["new.txt".into()])).unwrap();
    assert!(Status::read(&git).unwrap().is_clean());
    assert_eq!(fs::read_to_string(git.dir().join("a.txt")).unwrap(), "a\n");
}

#[test]
fn unstage_works_before_the_first_commit() {
    let (_dir, git) = repo();
    write(&git, "a.txt", "a\n");
    git.apply(&Op::Stage(vec!["a.txt".into()])).unwrap();
    git.apply(&Op::Unstage(vec!["a.txt".into()])).unwrap();
    assert_eq!(status(&git), [("a.txt".into(), None, Some(Change::Untracked))]);
}

#[test]
fn stage_and_unstage_single_lines() {
    let (_dir, git) = repo();
    commit(&git, "f.txt", "1\n2\n3\n4\n5\n", "Numbers");
    write(&git, "f.txt", "1\nTWO\n3\nFOUR\n5\n");

    let unstaged = diff::read(&git, &Source::Unstaged("f.txt".into())).unwrap();
    let two = unstaged.hunks[0].lines.iter().position(|l| l.text == "TWO").unwrap();
    let removed_two = unstaged.hunks[0].lines.iter().position(|l| l.text == "2").unwrap();
    let patch = unstaged.patch(|_, i| i == two || i == removed_two, false);
    git.apply(&Op::ApplyToIndex { patch, reverse: false }).unwrap();

    let changed = |source| {
        let diff: FileDiff = diff::read(&git, &source).unwrap();
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind != LineKind::Context)
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(changed(Source::Staged("f.txt".into())), ["2", "TWO"]);
    assert_eq!(changed(Source::Unstaged("f.txt".into())), ["4", "FOUR"]);

    let staged = diff::read(&git, &Source::Staged("f.txt".into())).unwrap();
    let patch = staged.patch(|_, _| true, true);
    git.apply(&Op::ApplyToIndex { patch, reverse: true }).unwrap();
    assert_eq!(changed(Source::Staged("f.txt".into())), Vec::<String>::new());
    assert_eq!(changed(Source::Unstaged("f.txt".into())), ["2", "TWO", "4", "FOUR"]);
}

#[test]
fn discard_single_lines_from_the_working_tree() {
    let (_dir, git) = repo();
    commit(&git, "f.txt", "1\n2\n3\n4\n5\n", "Numbers");
    write(&git, "f.txt", "1\nTWO\n3\n4\n5\n");
    git.apply(&Op::Stage(vec!["f.txt".into()])).unwrap();
    write(&git, "f.txt", "1\nTWO\n3\nFOUR\n5\nsix");

    let discard = |text: &str| {
        let unstaged = diff::read(&git, &Source::Unstaged("f.txt".into())).unwrap();
        git.apply(&Op::DiscardLines(unstaged.patch(|h, i| unstaged.hunks[h].lines[i].text == text, true))).unwrap();
        fs::read_to_string(git.dir().join("f.txt")).unwrap()
    };
    // A discarded removal comes back, a discarded addition goes.
    assert_eq!(discard("4"), "1\nTWO\n3\n4\nFOUR\n5\nsix");
    assert_eq!(discard("FOUR"), "1\nTWO\n3\n4\n5\nsix");
    assert_eq!(discard("six"), "1\nTWO\n3\n4\n5\n");
    // What was staged stays staged.
    assert_eq!(status(&git), [("f.txt".into(), Some(Change::Modified), None)]);
}

#[test]
fn diffs_of_untracked_files_and_commits() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "one\ntwo\n", "Root");
    write(&git, "new.txt", "x\ny\n");

    let untracked = diff::read(&git, &Source::Untracked("new.txt".into())).unwrap();
    assert!(untracked.hunks[0].lines.iter().all(|l| l.kind == LineKind::Added));
    assert_eq!(untracked.hunks[0].lines.len(), 2);

    let added = diff::read(&git, &Source::Commit { commit: root.clone(), path: "a.txt".into(), from: None }).unwrap();
    assert_eq!(added.hunks[0].lines.iter().map(|l| l.new).collect::<Vec<_>>(), [Some(1), Some(2)]);

    fs::write(git.dir().join("bin.dat"), [0_u8, 1, 2, 0]).unwrap();
    assert!(diff::read(&git, &Source::Untracked("bin.dat".into())).unwrap().binary);
}

#[test]
fn the_diff_of_a_file_a_commit_renamed_shows_only_what_changed() {
    let (_dir, git) = repo();
    commit(&git, "old.txt", "1\n2\n3\n4\n5\n", "Numbers");
    git.run(&["mv", "old.txt", "new.txt"]).unwrap();
    let renamed = commit(&git, "new.txt", "1\n2\n3\n4\n5\n6\n", "Rename and count on");

    let file = status::commit_files(&git, &renamed).unwrap().remove(0);
    assert_eq!((file.change, file.from.as_deref()), (Change::Renamed, Some("old.txt")));
    let diff = diff::read(&git, &Source::Commit { commit: renamed, path: file.path, from: file.from }).unwrap();
    assert!(diff.header.iter().any(|line| line == "rename from old.txt"));
    let lines = diff.hunks.iter().flat_map(|hunk| &hunk.lines);
    let changed: Vec<_> = lines.filter(|line| line.kind != LineKind::Context).map(|line| &line.text).collect();
    assert_eq!(changed, ["6"]);
}

#[test]
fn commit_amend_branch_and_checkout() {
    let (_dir, git) = repo();
    commit(&git, "a.txt", "a\n", "First");
    let first = head(&git);
    git.apply(&Op::Commit { message: "  Better first\n\n# a comment\n".into(), amend: true }).unwrap();
    assert_ne!(head(&git), first);
    assert_eq!(git.run(&["log", "-1", "--format=%B"]).unwrap().trim(), "Better first");

    git.apply(&Op::Branch { name: "topic".into(), at: "HEAD".into() }).unwrap();
    assert_eq!(Status::read(&git).unwrap().branch.as_deref(), Some("topic"));
    git.apply(&Op::Checkout("main".into())).unwrap();
    git.apply(&Op::DeleteBranch("topic".into())).unwrap();
    assert_eq!(git.run(&["branch", "--list", "topic"]).unwrap(), "");

    git.apply(&Op::Checkout(head(&git))).unwrap();
    assert_eq!(Status::read(&git).unwrap().branch, None);
}

#[test]
fn a_branch_counts_the_commits_no_other_ref_has() {
    let (_dir, git) = repo();
    commit(&git, "a.txt", "a\n", "Root");
    git.apply(&Op::Branch { name: "topic".into(), at: "HEAD".into() }).unwrap();
    commit(&git, "t.txt", "1\n", "One");
    commit(&git, "t.txt", "2\n", "Two");
    git.apply(&Op::Checkout("main".into())).unwrap();
    assert_eq!(git.only_on("main").unwrap(), 0);
    assert_eq!(git.only_on("topic").unwrap(), 2);

    // A tag, a remote branch or another branch keeps them, also one whose name starts the same.
    git.apply(&Op::Tag { name: "v1".into(), at: "topic~1".into() }).unwrap();
    assert_eq!(git.only_on("topic").unwrap(), 1);
    git.run(&["update-ref", "refs/remotes/origin/topic", "topic"]).unwrap();
    assert_eq!(git.only_on("topic").unwrap(), 0);
    git.run(&["update-ref", "-d", "refs/remotes/origin/topic"]).unwrap();
    git.apply(&Op::Branch { name: "topic-2".into(), at: "topic".into() }).unwrap();
    assert_eq!(git.only_on("topic").unwrap(), 0);
    git.apply(&Op::Checkout("main".into())).unwrap();
    git.apply(&Op::DeleteBranch("topic-2".into())).unwrap();

    // Deleting goes ahead anyway: the app asks first.
    git.apply(&Op::DeleteBranch("topic".into())).unwrap();
    assert_eq!(git.run(&["branch", "--list", "topic"]).unwrap(), "");
    assert!(git.only_on("topic").is_err());
}

#[test]
fn rebase_cherry_pick_revert_and_reset() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "a\n", "Root");
    let on_main = commit(&git, "m.txt", "m\n", "Main");
    git.apply(&Op::Branch { name: "topic".into(), at: root.clone() }).unwrap();
    commit(&git, "t.txt", "t\n", "Topic");

    git.apply(&Op::Rebase("main".into())).unwrap();
    assert_eq!(git.run(&["rev-parse", "HEAD~1"]).unwrap().trim(), on_main);

    git.apply(&Op::Checkout("main".into())).unwrap();
    git.apply(&Op::CherryPick("topic".into())).unwrap();
    assert!(git.dir().join("t.txt").exists());
    git.apply(&Op::Revert("HEAD".into())).unwrap();
    assert!(!git.dir().join("t.txt").exists());

    let tip = head(&git);
    git.apply(&Op::Reset { to: "HEAD~2".into(), mode: ResetMode::Soft }).unwrap();
    assert_eq!(status(&git), []);
    git.apply(&Op::Reset { to: tip.clone(), mode: ResetMode::Hard }).unwrap();
    git.apply(&Op::Reset { to: "HEAD~1".into(), mode: ResetMode::Mixed }).unwrap();
    // The index has the cherry-picked file again, the working tree still has it reverted.
    assert_eq!(status(&git), [("t.txt".into(), None, Some(Change::Deleted))]);
    git.apply(&Op::Reset { to: tip, mode: ResetMode::Hard }).unwrap();
    assert!(Status::read(&git).unwrap().is_clean());
}

/// What undo and redo would do now.
fn undo(git: &Git) -> Undo {
    undo::read(&Repo::open(git.dir()).unwrap(), &Status::read(git).unwrap()).unwrap()
}

fn what(step: Option<&Move>) -> Option<&str> {
    step.map(|step| step.what.as_str())
}

fn branch(git: &Git) -> String {
    git.run(&["branch", "--show-current"]).unwrap().trim().to_owned()
}

#[test]
fn undo_and_redo_walk_through_the_reflog() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "a\n", "Root");
    let second = commit(&git, "a.txt", "a\nb\n", "Second");
    git.apply(&Op::Branch { name: "feature".into(), at: root.clone() }).unwrap();

    // Back one action at a time: the checkout, then the commit, whose change is staged again.
    let steps = undo(&git);
    assert_eq!(what(steps.last.as_ref()), Some("checkout: moving from main to feature"));
    assert_eq!(steps.undone, None);
    git.apply(&steps.last.unwrap().op).unwrap();
    assert_eq!((branch(&git), head(&git)), ("main".into(), second.clone()));
    let steps = undo(&git);
    assert_eq!(what(steps.last.as_ref()), Some("commit: Second"));
    git.apply(&steps.last.unwrap().op).unwrap();
    assert_eq!(head(&git), root);
    assert_eq!(status(&git), [("a.txt".into(), Some(Change::Modified), None)]);
    assert_eq!(undo(&git).last, None, "the first commit cannot be undone");

    // Forward again, in the same order.
    let steps = undo(&git);
    assert_eq!(what(steps.undone.as_ref()), Some("commit: Second"));
    git.apply(&steps.undone.unwrap().op).unwrap();
    assert_eq!(head(&git), second);
    assert!(Status::read(&git).unwrap().is_clean());
    git.apply(&undo(&git).undone.unwrap().op).unwrap();
    assert_eq!((branch(&git), head(&git)), ("feature".into(), root));
    assert_eq!(undo(&git).undone, None);

    // A new action after an undo leaves nothing to redo.
    git.apply(&undo(&git).last.unwrap().op).unwrap();
    commit(&git, "b.txt", "b\n", "Third");
    let steps = undo(&git);
    assert_eq!((what(steps.last.as_ref()), steps.undone), (Some("commit: Third"), None));
}

#[test]
fn undo_a_reset_the_way_it_left_things_and_redo_it() {
    for mode in [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard] {
        let (_dir, git) = repo();
        let root = commit(&git, "a.txt", "a\n", "Root");
        write(&git, "a.txt", "a\nb\n");
        git.apply(&Op::Stage(vec!["a.txt".into()])).unwrap();
        let second = commit(&git, "new.txt", "new\n", "Second");
        git.apply(&Op::Reset { to: root.clone(), mode }).unwrap();
        let left = status(&git);

        git.apply(&undo(&git).last.unwrap().op).unwrap();
        assert_eq!(head(&git), second, "{mode:?}");
        assert!(Status::read(&git).unwrap().is_clean(), "{mode:?}");
        git.apply(&undo(&git).undone.unwrap().op).unwrap();
        assert_eq!((head(&git), status(&git)), (root, left), "{mode:?}");
    }
}

#[test]
fn a_rebase_is_one_action_and_undo_keeps_uncommitted_work() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "a\n", "Root");
    git.apply(&Op::Branch { name: "topic".into(), at: root }).unwrap();
    commit(&git, "t.txt", "1\n", "One");
    let topic = commit(&git, "t.txt", "2\n", "Two");
    git.apply(&Op::Checkout("main".into())).unwrap();
    commit(&git, "m.txt", "m\n", "Main");
    git.apply(&Op::Checkout("topic".into())).unwrap();
    // Made on the command line, like any other action.
    git.run(&["rebase", "-q", "main"]).unwrap();
    write(&git, "a.txt", "work in progress\n");

    let steps = undo(&git);
    assert_eq!(what(steps.last.as_ref()), Some("rebase"));
    git.apply(&steps.last.unwrap().op).unwrap();
    assert_eq!((branch(&git), head(&git)), ("topic".into(), topic));
    assert_eq!(status(&git), [("a.txt".into(), None, Some(Change::Modified))]);

    // Nothing while a rebase is under way; once aborted, the action before it.
    git.apply(&Op::Discard(vec!["a.txt".into()])).unwrap();
    commit(&git, "m.txt", "clash\n", "Clash");
    assert!(git.apply(&Op::Rebase("main".into())).is_err());
    assert_eq!(undo(&git), Undo::default());
    git.run(&["rebase", "--abort"]).unwrap();
    assert_eq!(what(undo(&git).last.as_ref()), Some("commit: Clash"));
}

#[test]
fn rebase_interactively_in_a_new_order_with_squash_fixup_and_drop() {
    let (_dir, git) = repo();
    let root = commit(&git, "root.txt", "root\n", "Root");
    let [one, two, three, four, five] =
        ["one", "two", "three", "four", "five"].map(|name| commit(&git, &format!("{name}.txt"), "\n", name));
    git.apply(&Op::Branch { name: "side".into(), at: root.clone() }).unwrap();
    git.apply(&Op::CherryPick(five.clone())).unwrap();
    git.apply(&Op::Checkout("main".into())).unwrap();

    let listed = |onto: &str| git.rebase_plan(onto).unwrap();
    assert_eq!(listed(&root), [&one, &two, &three, &four, &five].map(|sha| id(sha)));
    assert_eq!(listed("side"), [&one, &two, &three, &four].map(|sha| id(sha)), "side has a copy of five");
    assert_eq!(listed("HEAD"), [] as [ObjectId; 0]);

    let todo =
        [(Step::Pick, three), (Step::Squash, one), (Step::Drop, two), (Step::Pick, five.clone()), (Step::Fixup, four)];
    git.apply(&Op::RebaseInteractive { onto: root, todo: todo.into(), messages: BTreeMap::new() }).unwrap();
    assert_eq!(git.run(&["log", "--format=%B|"]).unwrap(), "five\n|\nthree\n\none\n|\nRoot\n|\n");
    assert_eq!(git.run(&["show", "--name-only", "--format="]).unwrap(), "five.txt\nfour.txt\n");
    assert_eq!(git.run(&["ls-files"]).unwrap(), "five.txt\nfour.txt\none.txt\nroot.txt\nthree.txt\n");

    let steps = undo(&git);
    assert_eq!(what(steps.last.as_ref()), Some("rebase"));
    git.apply(&steps.last.unwrap().op).unwrap();
    assert_eq!(head(&git), five);
}

#[test]
fn reword_in_an_interactive_rebase_keeps_the_author_also_after_a_conflict() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "root\n", "Root");
    let one = commit(&git, "a.txt", "one\n", "One");
    commit(&git, "a.txt", "two\n", "Two");
    git.run(&["commit", "--amend", "--no-edit", "--author=Ann Other <ann@example.com>", "--date=2001-02-03T04:05:06Z"])
        .unwrap();
    let two = head(&git);
    let three = commit(&git, "b.txt", "three\n", "Three");
    let four = commit(&git, "c.txt", "four\n", "Four");

    // Without one, two clashes with the root; the rebase stops before it rewords two.
    let todo = [(Step::Drop, one), (Step::Reword, two.clone()), (Step::Squash, three), (Step::Reword, four.clone())];
    let messages = [(two, "  Second\n\nWith a body\n\n".into()), (four, "Fourth".into())].into();
    let error = git.apply(&Op::RebaseInteractive { onto: root, todo: todo.into(), messages }).unwrap_err();
    assert!(error.to_string().contains("could not apply"), "{error}");
    write(&git, "a.txt", "two\n");
    git.apply(&Op::Stage(vec!["a.txt".into()])).unwrap();
    git.apply(&Op::Continue("rebase")).unwrap();

    assert_eq!(git.run(&["log", "--format=%B|"]).unwrap(), "Fourth\n|\nSecond\n\nWith a body\n\nThree\n|\nRoot\n|\n");
    assert_eq!(git.run(&["log", "-1", "--format=%an %at", "HEAD~"]).unwrap(), "Ann Other 981173106\n");
    assert_eq!(git.run(&["ls-files"]).unwrap(), "a.txt\nb.txt\nc.txt\n");
}

/// `main` and `topic` both change `a.txt`, and `main` deletes `b.txt`, which `topic` changes.
/// Returns the tips of `main`, which is checked out, and `topic`.
fn clashing(git: &Git) -> (String, String) {
    write(git, "b.txt", "b\n");
    git.apply(&Op::Stage(vec!["b.txt".into()])).unwrap();
    let root = commit(git, "a.txt", "a\n", "Root");
    git.apply(&Op::Branch { name: "topic".into(), at: root }).unwrap();
    write(git, "b.txt", "topic\n");
    git.apply(&Op::Stage(vec!["b.txt".into()])).unwrap();
    let topic = commit(git, "a.txt", "topic\n", "Topic");
    git.apply(&Op::Checkout("main".into())).unwrap();
    git.apply(&Op::Remove(vec!["b.txt".into()])).unwrap();
    (commit(git, "a.txt", "main\n", "Main"), topic)
}

fn underway(git: &Git) -> Option<Underway> {
    Repo::open(git.dir()).unwrap().underway()
}

fn conflicted(ours: bool, theirs: bool) -> Change {
    Change::Conflicted { ours, theirs }
}

fn read(git: &Git, path: &str) -> String {
    fs::read_to_string(git.dir().join(path)).unwrap()
}

#[test]
fn resolve_conflicts_and_continue_a_merge() {
    let (_dir, git) = repo();
    clashing(&git);
    assert!(git.apply(&Op::Merge("topic".into())).is_err());
    let merge = underway(&git).unwrap();
    assert_eq!((merge.command, merge.theirs, merge.message.as_str()), ("merge", "MERGE_HEAD", "Merge branch 'topic'"));
    assert_eq!(
        status(&git),
        [("a.txt".into(), None, Some(conflicted(true, true))), ("b.txt".into(), None, Some(conflicted(false, true)))]
    );
    assert_eq!(undo(&git), Undo::default(), "nothing to undo half way");

    assert!(git.apply(&Op::Continue("merge")).is_err(), "not before every conflict is resolved");
    git.apply(&Op::Restore { from: merge.theirs.into(), paths: vec!["a.txt".into()] }).unwrap();
    git.apply(&Op::Remove(vec!["b.txt".into()])).unwrap();
    assert_eq!(status(&git), [("a.txt".into(), Some(Change::Modified), None)]);
    git.apply(&Op::Continue("merge")).unwrap();
    assert_eq!(underway(&git), None);
    assert_eq!(git.run(&["rev-list", "--parents", "-n1", "HEAD"]).unwrap().split(' ').count(), 3, "a merge commit");
    assert_eq!(read(&git, "a.txt"), "topic\n");
    assert!(!git.dir().join("b.txt").exists());
}

#[test]
fn resolve_a_file_conflict_by_conflict() {
    let (_dir, git) = repo();
    let lines = |second: &str, last: &str| format!("1\n{second}\n3\n4\n5\n6\n7\n{last}\n");
    let root = commit(&git, "a.txt", &lines("2", "8"), "Root");
    git.apply(&Op::Branch { name: "topic".into(), at: root }).unwrap();
    commit(&git, "a.txt", &lines("topic", "topic"), "Topic");
    git.apply(&Op::Checkout("main".into())).unwrap();
    commit(&git, "a.txt", &lines("main", "main"), "Main");
    assert!(git.apply(&Op::Merge("topic".into())).is_err());

    let file = Conflicts::read(&git, "a.txt").unwrap();
    assert_eq!(file.names, ["HEAD".to_owned(), "topic".to_owned()]);
    assert_eq!(file.count(), 2);
    let text = file.resolve(&[Some(Pick::Theirs), Some(Pick::Both)]).unwrap();
    git.apply(&Op::Save { path: "a.txt".into(), text }).unwrap();
    assert_eq!(read(&git, "a.txt"), lines("topic", "main\ntopic"));
    assert_eq!(status(&git), [("a.txt".into(), Some(Change::Modified), None)]);
    git.apply(&Op::Continue("merge")).unwrap();
    assert_eq!(underway(&git), None);

    assert!(matches!(Conflicts::read(&git, "a.txt"), Err(Error::NoConflicts(path)) if path == "a.txt"));
    assert!(matches!(Conflicts::read(&git, "gone.txt"), Err(Error::File { .. })));
}

#[test]
fn skip_abort_and_continue_a_rebase_a_cherry_pick_and_a_revert() {
    let (_dir, git) = repo();
    let (main, topic) = clashing(&git);
    assert!(git.apply(&Op::Rebase("topic".into())).is_err());
    let rebase = underway(&git).unwrap();
    assert_eq!((rebase.command, rebase.theirs, rebase.message.as_str()), ("rebase", "REBASE_HEAD", "Main"));
    // Rebasing turns the sides around: the commit coming in is the one that deleted the file.
    assert_eq!(status(&git)[1], ("b.txt".into(), None, Some(conflicted(true, false))));
    git.apply(&Op::Skip("rebase")).unwrap();
    assert_eq!((underway(&git), branch(&git), head(&git)), (None, "main".into(), topic.clone()));
    git.apply(&Op::Reset { to: main.clone(), mode: ResetMode::Hard }).unwrap();

    assert!(git.apply(&Op::CherryPick("topic".into())).is_err());
    assert_eq!(underway(&git).map(|u| (u.command, u.theirs)), Some(("cherry-pick", "CHERRY_PICK_HEAD")));
    git.apply(&Op::Abort("cherry-pick")).unwrap();
    assert_eq!((underway(&git), head(&git)), (None, main));
    assert!(Status::read(&git).unwrap().is_clean());

    // Reverting "Main" brings back what came before it.
    commit(&git, "a.txt", "later\n", "Later");
    assert!(git.apply(&Op::Revert("HEAD~1".into())).is_err());
    let revert = underway(&git).unwrap();
    assert_eq!((revert.command, revert.message.as_str()), ("revert", "Revert \"Main\""));
    git.apply(&Op::Restore { from: revert.theirs.into(), paths: vec!["a.txt".into()] }).unwrap();
    assert_eq!(read(&git, "a.txt"), "a\n");
    git.apply(&Op::Continue("revert")).unwrap();
    assert_eq!(underway(&git), None);
    assert_eq!(git.run(&["log", "-1", "--format=%s"]).unwrap(), "Revert \"Main\"\n");
}

#[test]
fn stash_tags_and_errors() {
    let (_dir, git) = repo();
    commit(&git, "a.txt", "a\n", "Root");
    write(&git, "a.txt", "changed\n");
    write(&git, "new.txt", "new\n");
    git.apply(&Op::Stash("wip".into())).unwrap();
    assert!(Status::read(&git).unwrap().is_clean());
    git.apply(&Op::StashApply(0)).unwrap();
    git.apply(&Op::Discard(vec!["a.txt".into()])).unwrap();
    git.apply(&Op::DeleteUntracked(vec!["new.txt".into()])).unwrap();
    git.apply(&Op::StashPop(0)).unwrap();
    assert_eq!(status(&git).len(), 2);
    git.apply(&Op::Stash("again".into())).unwrap();
    assert_eq!(status::stashes(&git).unwrap(), ["On main: again"]);
    git.apply(&Op::StashDrop(0)).unwrap();
    assert_eq!(git.run(&["stash", "list"]).unwrap(), "");

    git.apply(&Op::Tag { name: "v1".into(), at: "HEAD".into() }).unwrap();
    git.apply(&Op::DeleteTag("v1".into())).unwrap();
    assert_eq!(git.run(&["tag"]).unwrap(), "");

    match git.apply(&Op::Checkout("no-such-branch".into())) {
        Err(Error::Git { args, message }) => {
            assert_eq!(args, "checkout -q no-such-branch");
            assert!(message.contains("no-such-branch"), "{message}");
        }
        other => panic!("expected a git error, got {other:?}"),
    }
}

#[test]
fn push_fetch_and_pull_through_a_remote() {
    let (_dir, git) = repo();
    let remote = tempfile::tempdir().unwrap();
    Git::new(remote.path()).run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
    git.run(&["remote", "add", "origin", remote.path().to_str().unwrap()]).unwrap();
    commit(&git, "a.txt", "a\n", "Root");
    git.apply(&Op::Push { remote: Some("origin".into()), track: true, force: false }).unwrap();
    assert_eq!(Status::read(&git).unwrap().upstream.as_deref(), Some("origin/main"));

    let other_dir = tempfile::tempdir().unwrap();
    let other = init(Git::new(other_dir.path()), &["clone", "-q", remote.path().to_str().unwrap(), "."]);
    commit(&other, "b.txt", "b\n", "From elsewhere");
    other.apply(&Op::Push { remote: None, track: false, force: false }).unwrap();

    git.apply(&Op::Fetch).unwrap();
    assert_eq!(Status::read(&git).unwrap().behind, 1);
    git.apply(&Op::Pull { rebase: false }).unwrap();
    let status = Status::read(&git).unwrap();
    assert_eq!((status.ahead, status.behind), (0, 0));
    assert!(git.dir().join("b.txt").exists());

    // After an amend the branch went its own way: only a force push gets it there.
    git.apply(&Op::Commit { message: "Amended".into(), amend: true }).unwrap();
    let status = Status::read(&git).unwrap();
    assert_eq!((status.ahead, status.behind), (1, 1));
    assert!(git.apply(&Op::Push { remote: None, track: false, force: false }).is_err());
    git.apply(&Op::Push { remote: None, track: false, force: true }).unwrap();
    assert_eq!(Git::new(remote.path()).run(&["rev-parse", "main"]).unwrap(), git.run(&["rev-parse", "HEAD"]).unwrap());

    // The lease refuses to overwrite the remote branch once it changed since the last fetch.
    Git::new(remote.path()).run(&["update-ref", "refs/heads/main", "main~1"]).unwrap();
    git.apply(&Op::Commit { message: "Amended again".into(), amend: true }).unwrap();
    assert!(git.apply(&Op::Push { remote: None, track: false, force: true }).is_err());
}

#[test]
fn pull_with_rebase_push_to_another_remote_and_push_tags() {
    let (_dir, git) = repo();
    let dirs = [(); 2].map(|()| tempfile::tempdir().unwrap());
    let [origin, fork] = [0, 1].map(|i| Git::new(dirs[i].path()));
    for (name, remote) in [("origin", &origin), ("fork", &fork)] {
        remote.run(&["init", "-q", "--bare", "-b", "main"]).unwrap();
        git.run(&["remote", "add", name, remote.dir().to_str().unwrap()]).unwrap();
    }
    let names: Vec<_> = git.remotes().unwrap().into_iter().map(|remote| remote.name).collect();
    assert_eq!(names, ["fork", "origin"]);
    let root = commit(&git, "a.txt", "a\n", "Root");
    git.apply(&Op::Push { remote: Some("origin".into()), track: true, force: false }).unwrap();

    // Someone else pushed a commit while this one was made: a pull with rebase puts it on top.
    commit(&git, "b.txt", "b\n", "From elsewhere");
    git.apply(&Op::Push { remote: None, track: false, force: false }).unwrap();
    git.apply(&Op::Reset { to: root, mode: ResetMode::Hard }).unwrap();
    commit(&git, "c.txt", "c\n", "Here");
    git.apply(&Op::Pull { rebase: true }).unwrap();
    assert_eq!(git.run(&["log", "--format=%s"]).unwrap(), "Here\nFrom elsewhere\nRoot\n");

    // Pushing, and force pushing, to another remote leaves the upstream where it is.
    git.apply(&Op::Push { remote: Some("fork".into()), track: false, force: false }).unwrap();
    git.apply(&Op::Commit { message: "Here, amended".into(), amend: true }).unwrap();
    git.apply(&Op::Push { remote: Some("fork".into()), track: false, force: true }).unwrap();
    assert_eq!(fork.run(&["rev-parse", "main"]).unwrap().trim(), head(&git));
    let status = Status::read(&git).unwrap();
    assert_eq!((status.upstream.as_deref(), status.ahead, status.behind), (Some("origin/main"), 1, 0));

    git.run(&["tag", "v1"]).unwrap();
    git.apply(&Op::PushTags("fork".into())).unwrap();
    assert_eq!(fork.run(&["tag"]).unwrap(), "v1\n");
    assert_eq!(origin.run(&["tag"]).unwrap(), "");
}

/// The remote branches, without the `origin/HEAD` that git 2.48 and later make when fetching.
fn remote_branches(git: &Git) -> Vec<String> {
    let refs = git.run(&["for-each-ref", "--format=%(refname:lstrip=2)", "refs/remotes"]).unwrap();
    refs.lines().filter(|name| !name.ends_with("/HEAD")).map(str::to_owned).collect()
}

#[test]
fn add_rename_repoint_and_remove_a_remote() {
    let (_dir, git) = repo();
    commit(&git, "a.txt", "a\n", "Root");
    let dirs = [(); 2].map(|()| tempfile::tempdir().unwrap());
    let [first, second] = [0, 1].map(|i| dirs[i].path().to_str().unwrap().to_owned());
    for dir in [&first, &second] {
        git.run(&["clone", "-q", "--bare", ".", dir]).unwrap();
    }
    Git::new(&second).run(&["branch", "other", "main"]).unwrap();
    let remote = |name: &str, url: &str| Remote { name: name.into(), url: url.into() };

    // A new remote is fetched from at once.
    git.apply(&Op::AddRemote { name: "origin".into(), url: first.clone() }).unwrap();
    assert_eq!(git.remotes().unwrap(), [remote("origin", &first)]);
    assert_eq!(remote_branches(&git), ["origin/main"]);
    git.run(&["branch", "-q", "--set-upstream-to=origin/main"]).unwrap();

    // Its remote branches and the upstream go along with a new name; a new URL fetches elsewhere.
    git.apply(&Op::RenameRemote { name: "origin".into(), to: "upstream".into() }).unwrap();
    git.apply(&Op::SetRemoteUrl { name: "upstream".into(), url: second.clone() }).unwrap();
    git.apply(&Op::Fetch).unwrap();
    assert_eq!(git.remotes().unwrap(), [remote("upstream", &second)]);
    assert_eq!(remote_branches(&git), ["upstream/main", "upstream/other"]);
    assert_eq!(Status::read(&git).unwrap().upstream.as_deref(), Some("upstream/main"));

    // A remote without a URL is listed too, and git says why it refuses a name.
    git.run(&["config", "remote.bare.fetch", "+refs/heads/*:refs/remotes/bare/*"]).unwrap();
    let error = git.apply(&Op::RenameRemote { name: "bare".into(), to: "upstream".into() }).unwrap_err();
    assert!(error.to_string().contains("already exists"), "{error}");
    assert_eq!(git.remotes().unwrap(), [remote("bare", ""), remote("upstream", &second)]);

    git.apply(&Op::RemoveRemote("upstream".into())).unwrap();
    assert_eq!(git.remotes().unwrap(), [remote("bare", "")]);
    assert_eq!(remote_branches(&git), Vec::<String>::new());
    assert_eq!(Status::read(&git).unwrap().upstream, None);
}

#[test]
fn init_and_clone_with_progress() {
    let dir = tempfile::tempdir().unwrap();
    let new = git::init(&dir.path().join("new/project")).unwrap();
    assert_eq!(Repo::open(new.dir()).unwrap().history().unwrap().ids.len(), 0);

    let (source_dir, source) = repo();
    let root = commit(&source, "a.txt", "a\n", "Root");
    // A file:// URL makes git send the objects, with progress, instead of copying the folder.
    let path = source_dir.path().to_str().unwrap().replace('\\', "/");
    let url = format!("file://{}{path}", if path.starts_with('/') { "" } else { "/" });
    let mut lines = Vec::new();
    let clone = git::clone(&url, &dir.path().join("clone"), |line| lines.push(line.to_owned())).unwrap();
    assert_eq!(head(&clone), root);
    assert!(lines.iter().any(|line| line.contains("100%")), "{lines:?}");

    match git::clone(&url, &dir.path().join("clone"), |_| {}) {
        Err(Error::Git { message, .. }) => assert!(message.contains("clone"), "{message}"),
        other => panic!("expected a git error, got {other:?}"),
    }
}

#[test]
fn clone_picks_the_folder_name_git_would() {
    for (url, name) in [
        ("https://github.com/Lars-/gitcraft.git", "gitcraft"),
        ("git@github.com:Lars-/gitcraft.git", "gitcraft"),
        ("https://example.com/repo/", "repo"),
        ("/path/to/repo/.git", "repo"),
        (r"C:\code\repo", "repo"),
        ("", ""),
    ] {
        assert_eq!(git::clone_name(url), name, "{url}");
    }
}

/// Watches the repository, with a message for every change it reports.
fn watch(git: &Git) -> (Watch, Receiver<()>) {
    let (send, changes) = mpsc::channel();
    let watch = Repo::open(git.dir())
        .unwrap()
        .watch(move || {
            // Sending fails only once the test is over.
            let _ = send.send(());
        })
        .unwrap();
    // macOS may still report changes made just before watching began.
    noticed(&changes, QUIET);
    (watch, changes)
}

/// Whether a change is reported within `time`. Forgets the rest of the burst, so the next
/// question starts afresh.
fn noticed(changes: &Receiver<()>, time: Duration) -> bool {
    let noticed = changes.recv_timeout(time).is_ok();
    std::thread::sleep(Duration::from_millis(200));
    while changes.try_recv().is_ok() {}
    noticed
}

const SOON: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(500);

#[test]
fn changes_made_outside_are_noticed() {
    let (_dir, git) = repo();
    commit(&git, "a.txt", "a\n", "Root");
    let (_watch, changes) = watch(&git);

    write(&git, "a.txt", "edited\n");
    assert!(noticed(&changes, SOON), "an edited file");
    git.run(&["add", "a.txt"]).unwrap();
    assert!(noticed(&changes, SOON), "staged in a terminal");
    git.run(&["branch", "feature"]).unwrap();
    assert!(noticed(&changes, SOON), "a new branch");
    git.run(&["checkout", "-q", "feature"]).unwrap();
    assert!(noticed(&changes, SOON), "another branch checked out");
}

#[test]
fn ignored_files_and_reading_are_not_noticed() {
    let (_dir, git) = repo();
    commit(&git, ".gitignore", "target/\n", "Ignore builds");
    fs::create_dir(git.dir().join("target")).unwrap();
    write(&git, "new.txt", "untracked\n");
    let repo = Repo::open(git.dir()).unwrap();
    let (_watch, changes) = watch(&git);

    write(&git, "target/build.log", "compiling\n");
    repo.history().unwrap();
    Status::read(&git).unwrap();
    status::stashes(&git).unwrap();
    diff::read(&git, &Source::Untracked("new.txt".into())).unwrap();
    assert!(!noticed(&changes, QUIET));

    write(&git, ".gitignore", "");
    assert!(noticed(&changes, SOON), "the ignore rules changed");
    write(&git, "target/build.log", "done\n");
    assert!(noticed(&changes, SOON), "no longer ignored");
}

#[test]
fn a_question_gets_its_answer_or_none_and_needs_the_token() {
    let (address, questions) = askpass::listen(|| {}).unwrap();
    for (prompt, secret, answer) in [
        ("Username for 'https://example.com': ", false, Some("tess")),
        ("Password for 'https://tess@example.com': ", true, Some("")),
        ("Enter passphrase for key '/home/tess/.ssh/id_ed25519': ", true, None),
        ("Are you sure you want to continue connecting (yes/no/[fingerprint])? ", false, Some("yes")),
    ] {
        let address = address.clone();
        let asking = std::thread::spawn(move || askpass::ask(&address, prompt).unwrap());
        let question = questions.recv_timeout(SOON).unwrap();
        assert_eq!((question.prompt.as_str(), question.secret()), (prompt, secret));
        question.answer(answer);
        assert_eq!(asking.join().unwrap().as_deref(), answer);
    }

    let (at, _token) = address.split_once('/').unwrap();
    assert_eq!(askpass::ask(&format!("{at}/guessed"), "Password: ").unwrap(), None);
    assert!(questions.recv_timeout(QUIET).is_err(), "asked without the token");
}
