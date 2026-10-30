# Contributing to GitCraft

Thanks for helping. GitCraft wants to be the fastest git client around, with a codebase small
enough to read in an afternoon. Everything below serves those two goals.

## The rules

1. **Every change in behaviour comes with a test.** A bug fix gets the test that would have caught
   it; a feature gets tests for what a user can do with it. Pull requests without tests are not
   merged.
2. **As little code as possible.** Before adding code, look for code to reuse or remove. Prefer one
   clear function over a framework, the standard library over a dependency, and a dependency over a
   reimplementation of something hard. A pull request that removes more lines than it adds is a
   good pull request.
3. **Never slower.** CI measures every pull request against `main` and fails when a benchmark gets
   more than 15% slower. Work that grows with the size of the history (every commit, every ref)
   needs a benchmark in `crates/core/benches`.
4. **No warnings.** `cargo fmt`, `cargo clippy` with the pedantic lints, and the docs all build
   without a single warning. Do not silence a lint with `allow` unless the code is clearer for it,
   and say why in a comment.
5. **Clean room.** GitCraft is written from scratch. Do not copy, translate or look at the code,
   icons or other assets of GitKraken or any other closed-source client, and do not decompile them.
   Behaviour you know from using such a client is fine to build; their implementation is not.

## Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

To measure speed yourself, point the benchmarks at a large repository:

```sh
git clone --bare --filter=blob:none https://github.com/git/git.git /tmp/git.git
GITCRAFT_BENCH_REPO=/tmp/git.git cargo bench -p gitcraft-core
```

Keep a pull request to one change. Describe what the user gets and how it is tested; the template
asks for both. Commit messages follow [Conventional Commits](https://www.conventionalcommits.org):
`fix: keep the selection after a fetch`, one line.

## How GitCraft is built

There are two crates.

**`crates/core` (`gitcraft-core`)** knows nothing about the screen.

- `repo.rs` reads the history, refs and commits with gitoxide, in-process, because the graph needs
  millions of lookups.
- `graph.rs` orders commits (`git log --date-order`) and lays out the lanes. It keeps a snapshot of
  the lanes every 256 rows, so it only ever builds the rows on screen.
- `git.rs` runs the user's own `git` for everything else. Every change to a repository is one
  variant of `Op`, which maps to the arguments for `git`; cloning and creating a repository are
  the two functions next to it. Using `git` itself keeps hooks, credentials, signing and config
  working without code of our own.
- `status.rs` and `diff.rs` parse `git status --porcelain=v2` and unified diffs, and build the
  patches for staging single lines.

Its tests run against real repositories made with `git` in a temporary folder
(`crates/core/tests`), plus property tests for the graph layout.

**`crates/app` (`gitcraft`)** is the window, built with [egui](https://github.com/emilk/egui).

- `app.rs` holds the state. Views never change it directly: they push an `Action`, which
  `Workspace::apply` carries out after the frame. Anything that touches the repository runs on a
  background thread and is followed by a reload, so the window never waits.
- `start.rs` (open, clone and create), `ui.rs` (tabs and toolbar), `graph.rs`, `sidebar.rs`, `details.rs`
  and `diff.rs` draw the views, and `theme.rs` has the colours.

The app's tests (`crates/app/src/tests.rs`) drive the whole window the way a user does, through
the accessibility tree with [egui_kittest](https://crates.io/crates/egui_kittest). That also keeps
GitCraft usable with a screen reader: give every new control an accessible name.

## Reporting bugs and asking for features

Open an issue with the templates. For a bug, say what you did, what happened and what you
expected, with your operating system and `git --version`. For anything about speed, say how big
the repository is. Security problems go through a private report instead; see
[SECURITY.md](SECURITY.md).

## Licence

Unless you say otherwise, any contribution you submit is licensed under both the MIT licence and the
Apache License 2.0, like GitCraft itself, without extra terms.
