# Terminal Replacement App — Design Brief

## What is this

A native app that replaces the terminal. It handles everything I do with text on my computer: running commands, reading files, writing/editing code, editing documents, monitoring running tasks. One app for all text-heavy interaction.

Not an IDE with a hundred panels. Not a raw terminal. Something in between — simple, fast, capable enough that I never need to leave it.

## How it works

### Multiple terminal sessions
You run multiple terminal sessions in split panes, like iTerm2 with split panes. You can have several running at once — AI coding sessions, dev servers, builds — and see them all.

### File tree
There is a file tree on the left side. It shows the cwd of whichever terminal is currently focused. When you switch focus to a different terminal, the file tree updates to show that terminal's working directory. Similar to how Warp's file tree follows cwd, but always scoped to the focused terminal.

### Opening files
When you click a file in the tree, or click a file link in the terminal output, a file view opens. This is a real editor and renderer:
- For code: a proper editor with syntax highlighting, like VS Code's editor. You can read and edit.
- For markdown: rendered view, not raw source.

The file view takes space from the terminals you're not actively using — not from the terminal you're focused on.

### Layout behavior
The focused terminal keeps its space when a file opens. The unfocused terminals yield space to make room for the file view. The exact mechanics of how this rearrangement works (how unfocused terminals compress, where things position) is still open for design exploration.

## Non-negotiable properties

- **Native app.** Not Electron, not a web app. Must be fast.
- **Replaces the terminal.** This is the only app you open for terminal work. It is the terminal.
- **Real editor.** The file editing experience should be on par with VS Code's editor — syntax highlighting, proper editing, not a toy.
- **Real renderer.** Markdown files render as formatted documents, not raw text.
- **Multi-session.** Running 4-6 terminal sessions simultaneously is the normal use case, not an edge case.

## Context: how I work today

- I use iTerm2 with 4-6 split panes running Claude Code sessions, dev servers, builds simultaneously.
- I constantly leave the terminal to open VS Code for editing, browsers for docs, separate viewers for files.
- I built a floating file viewer (Nobs Editor) that pops up above the terminal when I click file links — so I can read files without losing terminal context. This is a band-aid.
- When I need to read long terminal output, I maximize one pane, read, then shrink it back. Also a band-aid.
- The core friction: monitoring multiple tasks needs many small panes. Reading/editing anything needs a big view. These compete for the same screen space. This app should resolve that tension naturally.

## Open design questions

- How exactly do unfocused terminals compress when a file view opens? Do they get narrower? Collapse to a minimal indicator? Something else?
- Does the focused terminal's position stay fixed, or can it move as the layout rearranges?
- What about diffs — is there a diff viewer, or is that just the editor?
- File tree behavior when no file is open — always visible, or toggle?
- Tabs / multiple open files — how does that work?
- What is the right tech stack for a fast native app with a real code editor and terminal emulator? (Swift + AppKit? Rust + GPU rendering? Something else?)

## Reference points

- **Warp**: Good UI, has a file tree that follows cwd. But its workspace/directory management is confusing when running multiple sessions — directories pile up in the sidebar disconnected from terminals.
- **Zen Editor**: Close to the right idea but doesn't nail it.
- **Obsidian**: The "one place for everything" feel is the goal. But the vault concept (one big vault vs. many small vaults) is a friction point — this app shouldn't force that kind of organizational decision.
- **VS Code**: The editor quality is the bar. But VS Code is an IDE — too much, too heavy, terminal is secondary.
- **iTerm2**: The multi-pane terminal experience is the bar. But iTerm2 can only show raw text.
