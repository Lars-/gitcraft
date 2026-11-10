<p align="center">
  <img src="assets/icon/256.png" width="112" height="112" alt="GitCraft" />
</p>

<h1 align="center">GitCraft</h1>

<p align="center">
  <strong>A fast, open-source graphical git client, written in pure Rust.</strong><br />
  See every branch at once in a commit graph, stage single lines, commit, branch, merge, rebase
  and push, on Windows, macOS and Linux. Big repositories open instantly.
</p>

<p align="center">
  <a href="https://github.com/Lars-/gitcraft/actions/workflows/ci.yml"><img src="https://github.com/Lars-/gitcraft/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="#licence"><img src="https://img.shields.io/badge/licence-MIT%20%2F%20Apache--2.0-blue" alt="Licence: MIT or Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/status-early-d69e2e" alt="Status: early" />
</p>

![GitCraft with two repositories open in tabs: branches on the left, the commit graph with branch and tag labels in the middle, and the uncommitted changes with the commit form on the right](docs/screenshot.png)

## Fast

GitCraft reads the whole history in-process with [gitoxide](https://github.com/GitoxideLabs/gitoxide)
and only lays out and reads the rows on screen, so the size of a repository hardly matters.
Measured on [git.git](https://github.com/git/git) (85,875 commits, every branch and tag):

| | GitCraft | for comparison |
| --- | --- | --- |
| Open the whole history and lay out the graph | 0.11 s | `git rev-list --all --date-order`: 0.09 s |
| The same without a commit-graph file | 0.61 s | `git rev-list`: 0.59 s |
| Read and lay out one screen of commits | 0.1 ms | |
| Lay out 200,000 commits on 12 branches | 28 ms | |

Measured with `cargo bench` on an Intel Core i9-14900K. Every pull request runs the same
benchmarks against the main branch, and one that makes GitCraft slower does not get merged.

## What works

GitCraft is young. This is what it does today; [ROADMAP.md](ROADMAP.md) has what comes next.

- **Start**: open a folder, clone from a URL with git's progress as it goes, or create a new
  repository.
- **Tabs**: keep several repositories open at once; they open again where you left them.
- **Live**: what you change in an editor, a build or a terminal shows up by itself; files git
  ignores do not wake it.
- **Graph**: every branch, remote branch and tag in one graph, children above parents, with
  coloured lanes, branch and tag labels and the uncommitted changes on top. Arrow keys walk it.
- **Commits**: message, author, parents and the files they changed, with the diff of each file.
- **Staging**: stage or unstage files, whole hunks or single lines, then commit or amend.
- **Discard**: throw away the changes to a file, a hunk or single lines, after you confirm.
- **Branches and tags**: create, check out, merge, rebase, delete, from the sidebar or a right click.
- **Commits from the graph**: check out, cherry-pick, revert, reset (soft, mixed, hard), copy the SHA.
- **Interactive rebase**: right-click a commit to rebase onto it, and put the commits after it in
  a new order (with the arrows or by dragging), give them a new message, squash or fix them up
  into the one below, or drop them, before you start. Or drag a commit in the graph onto another
  to move it to its place, or pick "Reword commit…" from its menu: the rebase opens with that done.
- **Remotes**: fetch, pull and push, with how far ahead and behind you are. A branch that went its
  own way, after an amend or a rebase, is force pushed with a lease, so nobody else's work is lost.
  Right-click Pull to pull with rebase, or Push to push to another remote, force push or push your
  tags.
- **Passwords**: when git or ssh needs a username, a password or the passphrase of your key, for a
  clone, a fetch, a push or a signed commit, GitCraft asks for it in a dialog. With a credential
  helper set up, git remembers what you type, as it does on the command line.
- **Stashes**: stash, apply, pop and drop.
- **Conflicts**: when a merge, rebase, cherry-pick or revert stops at a conflict, a bar says so and
  counts the conflicted files. Open a conflicted file to go through it conflict by conflict, with
  your side and theirs next to each other and the result below: take ours, theirs or both, and
  save. Or take a whole file from one side (also when one side deleted it), or edit it and mark
  it resolved; then continue, skip the commit, or abort.
- **Undo and redo**: take back a commit, an amend, a checkout, a merge, a pull, a rebase, a
  cherry-pick, a revert or a reset, one step at a time, and do it again. It reads git's reflog, so
  it works for what you did on the command line too. Undoing a commit puts its changes back in the
  index; your uncommitted work stays where it is.
- **Safe by default**: a hard reset, deleting a branch with commits no other branch or tag has, a
  force push and dropping a stash all ask first, saying what will be lost.

GitCraft runs your own `git` for everything that changes a repository, so your config, hooks,
credentials and commit signing work exactly as they do on the command line.

## Install

GitCraft needs [git](https://git-scm.com/downloads) 2.32 or newer on your `PATH`.

Builds for Windows, macOS and Linux (x86-64 and ARM) come with each
[release](https://github.com/Lars-/gitcraft/releases). To build it yourself, install
[Rust](https://rustup.rs) and run:

```sh
cargo install --git https://github.com/Lars-/gitcraft gitcraft
gitcraft path/to/repository
```

## Contributing

Contributions are very welcome: bug reports, ideas and pull requests. Read
[CONTRIBUTING.md](CONTRIBUTING.md) first; the short version is that every change comes with tests,
does what it does in as little code as possible, and does not make GitCraft slower.

## Licence

GitCraft is licensed under either the [MIT licence](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. Unless you say otherwise, any contribution
you submit is licensed the same way, without extra terms.

GitCraft is an independent project and is not affiliated with GitKraken or Axosoft.
