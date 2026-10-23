<div align="center">

<img src="assets/yegz-icon.png" alt="Yegz icon" width="160" />

# Yegz

### A powerful editor. Without the clutter.

**A lightweight, Rust-powered workspace for code, text, and everything in between.**

[Website (yegz.com)](https://yegz.com) · [Features](#features) · [Getting started](#getting-started) · [Roadmap](#roadmap)

</div>

---

## About

**Yegz** is an Atom-inspired desktop editor built for people who want useful development tools without the weight of a full IDE.

Open a single file or explore a project, search through source code, edit Markdown, inspect images and PDFs, and keep track of Git changes—all from one focused interface.

Yegz is built in **Rust**, using **egui/eframe** for its native desktop UI.

> **Project status:** Early development. Features and platform support may change as the editor evolves.

## Features

| Area | Capabilities |
| --- | --- |
| **Editing** | Syntax highlighting, multiple tabs, line numbers, find and replace, regex and case-sensitive search |
| **Editing shortcuts** | Toggle comments, duplicate lines, move lines, and other keyboard-driven operations |
| **Projects** | File-tree sidebar, folder navigation, drag-and-drop file opening |
| **Quick navigation** | Fuzzy file finder and project-wide search with jump-to-result |
| **Git awareness** | Repository and file-status integration |
| **Markdown** | Rendered Markdown preview and editing modes |
| **File viewing** | Built-in image and PDF viewing |
| **Personalisation** | One Dark, One Light, GitHub Dark, and Monokai themes |
| **Workspace continuity** | Restore previously opened files, project folder, and session settings |
| **Unsaved work** | Modified-file indicators and confirmation before discarding changes |

Yegz aims to keep the everyday workflow fast and approachable—not to replace a full-featured IDE.

## Getting started

### Prebuilt downloads

Packaged releases are **not yet confirmed**. When builds are published, they will appear on the [Releases page](https://github.com/Zigl/yegz/releases).

For now, build Yegz from source.

### Build requirements

- **Rust toolchain** with `cargo` (install via [rustup](https://rustup.rs/)).
- A supported desktop operating system and graphics environment for `eframe`.
- **Git**, if you want repository-status features.
- Platform-specific native development dependencies may be needed by the crates in the project.

> The current source includes macOS-specific integrations (such as “Reveal in Finder”). macOS is the primary development target; Windows and Linux support have **not** been verified.

### Build from source

```bash
git clone https://github.com/Zigl/yegz.git
cd yegz
cargo run --release
```

To compile without launching:

```bash
cargo build --release
```

The executable is normally written to `target/release/` under the name configured in `Cargo.toml`.

**Note:** These commands assume the repository contains the Rust project manifest (`Cargo.toml`) and source tree. Make sure both are committed before publishing.

### Open a file or folder

The application accepts an optional file or directory argument:

```bash
cargo run --release -- /path/to/project
```

Or launch Yegz and use its **Open File** / **Open Folder** actions.

## Keyboard shortcuts

The following shortcuts are implemented in the current source. The symbols below reflect the macOS-focused UI.

| Action | Shortcut |
| --- | --- |
| Quick open / fuzzy finder | `⌘ P` |
| Search in project | `⌘ ⇧ F` |
| Find in file | `⌘ F` |
| Find next / previous | `⌘ G` / `⌘ ⇧ G` |
| Open file / folder | `⌘ O` / `⌘ ⇧ O` |
| Save / Save As | `⌘ S` / `⌘ ⇧ S` |
| New file | `⌘ N` |
| Close tab | `⌘ W` |
| Toggle sidebar | `⌘ B` |
| Settings | `⌘ ,` |
| Toggle comment | `⌘ /` |
| Duplicate line | `⌘ ⇧ D` |
| Move line | `⌥ ↑` / `⌥ ↓` |
| Next / previous tab | `Ctrl Tab` / `Ctrl ⇧ Tab` |
| Cycle Markdown mode | `Ctrl ⇧ M` |

## Built with

- **Rust** — application language
- **egui / eframe** — desktop user interface
- **egui_extras** — syntax highlighting and image-loading support
- **egui_commonmark** — Markdown rendering

Additional dependencies and exact versions are defined by the project's `Cargo.toml`.

## Roadmap

This is a **proposed roadmap**, not a promise of delivery dates.

- [x] Multi-tab editor and syntax highlighting
- [x] Project tree and fuzzy file navigation
- [x] Project-wide search
- [x] Git status integration
- [x] Markdown preview
- [x] Image and PDF viewers
- [x] Themes and session restoration
- [ ] Publish versioned releases
- [ ] Package and test macOS builds
- [ ] Validate Windows and Linux compatibility
- [ ] Improve large-file performance and editing ergonomics
- [ ] Add configurable keyboard shortcuts
- [ ] Expand Git workflows (diffs and change review)
- [ ] Explore language-server support and richer code intelligence
- [ ] Add an optional extension or plugin system

## Contributing

Yegz is in early development. Bug reports, reproducible issues, and suggestions are welcome via [GitHub Issues](https://github.com/Zigl/yegz/issues).

If you plan to accept external contributions, add a `CONTRIBUTING.md` with setup instructions and pull-request guidelines.

## License

**License not yet selected.** Until a licence is added, the repository does not grant general permission to copy, modify, or redistribute its contents. Choose and add a licence before presenting Yegz as open source.

---

<div align="center">

**Yegz — Your workspace, simplified.**

Made with Rust.

</div>
