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
- [x] Conflicts, part 2: resolve a conflicted file conflict by conflict, with ours and theirs side
      by side and the result below; take ours, theirs or both for each conflict, then save
- [x] Interactive rebase, part 1: from a commit's menu, the commits after it in a list, newest
      first; move them up and down, pick, squash, fix up or drop each, then start
- [x] Interactive rebase, part 2: reword a commit with a new message, and reorder the list by
      dragging
- [x] Interactive rebase, part 3: drag a commit in the graph onto another to move it to its place,
      and reword a commit from its menu; both open the rebase with that done, to start it
- [x] Credentials: ask for a username, password or passphrase when git or ssh needs one, and
      whether to trust a host ssh does not know yet
- [x] Push and pull options: pull with rebase, and push to any remote, force push or push tags,
      from the right-click menus of Pull and Push
- [x] Remotes: each remote in the sidebar with its branches; add one (and fetch it), give it
      another name or URL, or remove it
- [x] Search the graph by message, author or SHA, and jump to any branch or tag by its name
- [x] File history: the commits that changed a file, followed through renames, each with its diff
- [x] Blame: who last changed each line of a file, and in which commit
- [x] Better diffs, part 1: the words that changed within a line stand out
- [x] Better diffs, part 2: the old and the new version side by side
- [x] Better diffs, part 3: a picture's old and new version next to each other
- [x] Better diffs, part 4: code in the colours of its language
- [x] Keyboard, part 1: a command palette with everything the tabs, the toolbar and the selected
      commit offer, and shortcuts for the common commands
- [x] Keyboard, part 2: shortcuts to stage, unstage or discard the file or the lines in view, to
      go to the next and the previous file and back to the graph; the start screen's clone and
      create in the palette
- [x] Settings, part 1: a settings window (also Ctrl+,) to zoom, to pick how dates read, and
      whether Pull merges or rebases; they last between runs
- [x] Settings, part 2: a light theme beside the dark one, or the one the system uses; the
      graph, diffs, code and conflicts take colours made for it
- [x] Graph polish, part 1: names over the columns of the graph, with borders to drag to make the
      branches and tags, the graph and the author wider or narrower (a double click puts one back);
      they last between runs
- [x] Graph polish, part 2: compact lanes: lines that head for the same commit share a lane, and
      the lanes close up when a line ends, each line keeping its colour
- [x] Graph polish, part 3: an avatar beside each author, in the graph, a commit, a file's history
      and blame: their initials on a colour of their own, made on the spot and never fetched over
      the network; the settings hide them
- [x] Packaging, part 1: for Linux a Debian package and an AppImage, with the icon and a menu entry
- [x] Packaging, part 2: an installer for Windows (MSI) that puts GitCraft in the Start menu, and
      GitCraft.app in a disk image for macOS, both with the icon; not yet signed with a
      certificate, so the README says how to open them

## Next

1. **Packaging**, part 3: a Flatpak for Linux, with the icon and a menu entry.

## After 1.0

- Submodules, worktrees and Git LFS.
- Authors' pictures from a service such as Gravatar, only when turned on.
