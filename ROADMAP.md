# Roadmap

GitCraft 1.0 covers the git part of a full graphical client: everything you do with a repository
day to day, without leaving the app. Integrations with hosting services (pull requests, issues)
are not part of 1.0.

Items are worked on roughly in this order. Each is done when it works in the app, has tests that
drive it the way a user does, and keeps every benchmark as fast as before.

## Done

- [x] Open a repository: folder dialog, typed path, recent repositories, command-line argument
- [x] Commit graph of every branch, remote branch and tag, with lanes, labels and the uncommitted
      changes; keyboard navigation
- [x] Commit details with changed files and their diffs
- [x] Stage and unstage files, hunks and single lines; commit and amend
- [x] Branches and tags: create, check out, merge, rebase, delete
- [x] From the graph: check out, cherry-pick, revert, reset (soft, mixed, hard), copy the SHA
- [x] Fetch, pull and push, with ahead and behind counts
- [x] Stash, apply, pop and drop
- [x] Refresh when the window gets focus
- [x] Clone and init from the start screen, with progress
- [x] Tabs: several repositories open at once, remembered between runs
- [x] Live refresh: watch the working tree and refs, and reload by itself
- [x] Discard changes per file, hunk or line, after a confirmation
- [x] Confirm destructive actions: hard reset, deleting a branch with commits no other branch or tag
      has, force push (with lease, for a branch that went its own way), dropping a stash
- [x] Undo and redo through the reflog: commits, amends, checkouts, merges, pulls, rebases,
      cherry-picks, reverts and resets, also those made on the command line, several steps deep
- [x] Conflicts, part 1: a bar while a merge, rebase, cherry-pick or revert is stopped, with the
      number of conflicted files; continue, skip or abort; per file take ours or theirs (also when
      that side deleted it) or mark it resolved

## Next

1. **Conflicts, part 2**: resolve a conflicted file in a three-way view (ours, theirs, result),
   conflict by conflict.
2. **Interactive rebase**: reorder, squash, fix up, reword and drop commits, by dragging in the
   graph.
3. **Credentials**: ask for a username, password or passphrase when git needs one.
4. **Push and pull options**: force push on request, push tags, pick the remote, pull with rebase.
5. **Remotes**: add, edit and remove.
6. **Search the graph** by message, author or SHA, and jump to any branch or tag.
7. **File history and blame.**
8. **Better diffs**: side by side, word-level changes, syntax colouring, images.
9. **Keyboard**: shortcuts for everything, and a command palette.
10. **Settings**: light and dark theme, font size, date format, default pull mode.
11. **Graph polish**: resizable columns, compact lanes, author avatars (optional, never by default
    over the network).
12. **Packaging**: installers with the icon for Windows (MSI), macOS (.app in a .dmg) and Linux
    (AppImage, .deb, Flatpak), signed where the platform expects it.

## After 1.0

- Submodules, worktrees and Git LFS.
