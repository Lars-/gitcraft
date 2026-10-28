# Notes for coding agents

Read [CONTRIBUTING.md](CONTRIBUTING.md) first: its rules apply to every change, whoever writes it.
In short:

- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` before you finish. All three must pass without warnings.
- Every change in behaviour gets a test. Core behaviour is tested against real repositories in
  `crates/core/tests`; app behaviour through the accessibility tree in `crates/app/src/tests.rs`.
- Keep the code small. Reuse what is there, remove what your change makes unnecessary, and do not
  add a dependency for something a few lines can do.
- Views push an `Action`; only `Workspace::apply` changes state. Changes to a repository are `Op`
  variants run by the user's own `git`; reading the history goes through gitoxide.
- Work that grows with the size of the history needs a benchmark in `crates/core/benches`, and must
  not make an existing one slower.
- Never copy from GitKraken or any other closed-source client (see "Clean room" in CONTRIBUTING.md).
- Commit messages: Conventional Commits, one line.
