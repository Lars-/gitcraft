//! End-to-end tests against real repositories made with the installed `git`.

use std::fs;

use gitcraft_core::diff::{self, FileDiff, LineKind, Source};
use gitcraft_core::git::{Git, Op, ResetMode};
use gitcraft_core::repo::{Label, RefKind, Repo};
use gitcraft_core::status::{self, Change, Status};
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
fn diffs_of_untracked_files_and_commits() {
    let (_dir, git) = repo();
    let root = commit(&git, "a.txt", "one\ntwo\n", "Root");
    write(&git, "new.txt", "x\ny\n");

    let untracked = diff::read(&git, &Source::Untracked("new.txt".into())).unwrap();
    assert!(untracked.hunks[0].lines.iter().all(|l| l.kind == LineKind::Added));
    assert_eq!(untracked.hunks[0].lines.len(), 2);

    let added = diff::read(&git, &Source::Commit { commit: root.clone(), path: "a.txt".into() }).unwrap();
    assert_eq!(added.hunks[0].lines.iter().map(|l| l.new).collect::<Vec<_>>(), [Some(1), Some(2)]);

    fs::write(git.dir().join("bin.dat"), [0_u8, 1, 2, 0]).unwrap();
    assert!(diff::read(&git, &Source::Untracked("bin.dat".into())).unwrap().binary);
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
    git.apply(&Op::Push { upstream: Some("origin".into()), force: false }).unwrap();
    assert_eq!(Status::read(&git).unwrap().upstream.as_deref(), Some("origin/main"));

    let other_dir = tempfile::tempdir().unwrap();
    let other = init(Git::new(other_dir.path()), &["clone", "-q", remote.path().to_str().unwrap(), "."]);
    commit(&other, "b.txt", "b\n", "From elsewhere");
    other.apply(&Op::Push { upstream: None, force: false }).unwrap();

    git.apply(&Op::Fetch).unwrap();
    assert_eq!(Status::read(&git).unwrap().behind, 1);
    git.apply(&Op::Pull { rebase: false }).unwrap();
    let status = Status::read(&git).unwrap();
    assert_eq!((status.ahead, status.behind), (0, 0));
    assert!(git.dir().join("b.txt").exists());
}
