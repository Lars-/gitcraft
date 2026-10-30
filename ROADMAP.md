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

## Next

1. **Live refresh**: watch the working tree and refs, and reload by itself.
2. **Discard** changes per file, hunk or line.
3. **Confirm destructive actions**: hard reset, deleting a branch with unmerged commits, force push.
4. **Undo and redo** of the last action, through the reflog.
5. **Conflicts**: list conflicted files, resolve them in a three-way view (ours, theirs, result),
   mark them resolved, and continue or abort a merge, rebase or cherry-pick.
6. **Interactive rebase**: reorder, squash, fix up, reword and drop commits, by dragging in the
   graph.
7. **Credentials**: ask for a username, password or passphrase when git needs one.
8. **Push and pull options**: force with lease, push tags, pick the remote, pull with rebase.
9. **Remotes**: add, edit and remove.
10. **Search the graph** by message, author or SHA, and jump to any branch or tag.
11. **File history and blame.**
12. **Better diffs**: side by side, word-level changes, syntax colouring, images.
13. **Keyboard**: shortcuts for everything, and a command palette.
14. **Settings**: light and dark theme, font size, date format, default pull mode.
15. **Graph polish**: resizable columns, compact lanes, author avatars (optional, never by default
    over the network).
16. **Packaging**: installers with the icon for Windows (MSI), macOS (.app in a .dmg) and Linux
    (AppImage, .deb, Flatpak), signed where the platform expects it.

## After 1.0

- Submodules, worktrees and Git LFS.
