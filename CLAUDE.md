# Vale

Vale is a GIS for fictional worlds, for desktop and iPad. You draw the world in the app, on a globe. The design is in `docs/DESIGN.md`. Read it before you plan or write code.

## Current phase

The labeling prototype is done. The `vale-labeler` library labels points and lines from Natural Earth data, and the demo writes a PNG. This command makes the PNG:

```sh
cargo run --release -p vale-labeler-demo -- --preset world --out target/demo/world.png
```

An app prototype also exists. It is a vertical slice through phases 2, 4, 6, 8, and 9, ahead of the phase order. None of those phases is done. The app opens on the globe workspace, and a switch in the tool bar opens the flat map. This command opens the window:

```sh
cargo run --release -p vale-app
```

This command renders the full UI to a PNG without a window:

```sh
cargo run --release -p vale-app -- --screenshot target/app/ui.png --size 1440x900 --report
```

This command renders the iPad layout of the globe workspace to a PNG:

```sh
cargo run --release -p vale-app -- --layout pad --panel layers --screenshot target/app/pad.png --size 1194x834
```

This command builds the app and starts it on a connected iPad:

```sh
scripts/app-ipad.sh
```

A globe prototype also exists, in `vale-globe-proto`. It tests pen drawing on a globe and Apple Pencil input through egui, ahead of phase 1. A first test on an iPad showed smooth painting, pressure, and palm rejection. The app gets 120 pen samples per second and no hover events, as the winit source predicted. `crates/vale-globe-proto/README.md` has the test list. The first command opens the window on the desktop. The second command builds the app and starts it on a connected iPad:

```sh
cargo run --release -p vale-globe-proto
scripts/globe-proto-ipad.sh
```

The app prototype leaves out project files, linked sources, rule-based styles, polygon labels, and the atlas. It also leaves out conic and transverse projections, editing tools, and the GPU Vello backend. `README.md` has the full list.

The current phase is phase 0, the iPad test loop and CI. When each PR arrives on the iPad through TestFlight, and CI builds all four platforms, the phase is done.

The phase table in `docs/DESIGN.md` gives the order of the work after that. When a phase is done, update this section.

## Rules

- The project is one Rust workspace. Put each crate in `crates/`, and give each crate a `vale-` prefix.
- `vale-labeler` must not depend on any other Vale crate. It takes page-space geometry and returns placed text.
- The labeling engine is a clean-room design. Use published research and the public Esri documentation only. Do not read or decompile Esri code.
- The license is `MIT OR Apache-2.0` for all crates.
- Do not name a binary `vale`, because a popular prose linter uses that command name.
- Before you add Vello, Parley, or Krilla as a dependency, make sure of its current release status.
- Procedural world generation is a permanent non-goal.
- If a change contradicts `docs/DESIGN.md`, update the document in the same change.
- Reference files for the demo are updated with `VALE_UPDATE_REFERENCE=1`.

## Work tracking

- GitHub issues track the work. Issue #1 is the epic for version 1. Each phase is an issue, and its tasks are its sub-issues.
- Each issue has a list that starts with "This task is done when" or "This phase is done when". That list is the definition of done.

## Implementing changes

- Commit as you go, in small chunks that you can test. Do not make one large commit at the end of a feature.
- Keep the work inside the "done when" list of the issue. A small fix that you see on the way can go in the same PR.
- Write one test for each item of the "done when" list that a test can prove.
- Before you update a reference file, look at the new output. Review a changed reference file as you review code.

## Context usage

A feature session keeps its context small on purpose.

- If a lookup spans crates, give it to a subagent. Read only the files that you are about to change.
- Before the first edit, find each design decision that the issue and `docs/DESIGN.md` leave open. Plan it one time, with a planning subagent.
- If a feature spans more than one crate, give each crate to one subagent. If another crate depends on a crate, commit that crate first. Independent crates can run in separate worktrees at the same time.
- The lead session commits, makes sure that each item of the "done when" list is true, and opens the PR. A subagent returns a diff or a plan. It does not own the definition of done.
- Do not start a subagent for one lookup or for an edit to one file.
- Run a command with long output in the background. Examples are `cargo build`, `cargo test`, `cargo clippy`, and `gh run watch`. Bring only the lines that fail into the context.
- Read a generated file or a data file only at the section that changes. Examples are a reference file and Natural Earth data. Before you open a changed reference file, look at `git diff --stat`.
- After you edit a file, do not read it again. If the change does not apply, Edit and Write fail with an error.
- Do not read an unrelated crate "to be safe".
- After the tests pass and a commit lands, compact the session. Do not wait for an automatic compaction in the middle of a task.

## Pull requests

- Use one branch for each issue: `feat/<issue>-slug`, `fix/<issue>-slug`, or `chore/<issue>-slug`.
- Write `Closes #N` in the PR description. Before you ask for review, make sure that each item of the "done when" list is true.
- Do not wrap PR and issue bodies. Write one line for each paragraph and each list item, with blank lines between them. GitHub shows a single newline as a line break.
- Before you push, run these commands and make sure that all three succeed:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets
  cargo test --workspace
  ```

- If the PR has CI runs, watch them with `gh run watch`. After they pass, ask for review.
- Claude does not merge. A person reviews and merges each PR.
- Do not force-push `main`. While the PR of a feature branch is open, you can rewrite the history of that branch.
- Do not add Co-Authored-By trailers to commits.

## Documentation rules

These rules apply to code comments and to all documentation: this file, `docs/`, and the README files.

Do these things:

- Keep the text short.
- Write statements of how things are.
- Before prose lands, apply the `simple-english` skill to it. This includes comments, `docs/`, the README files, and PR and issue bodies.

Do not do these things:

- If the reason for something is obvious, do not document it.
- Do not repeat what the code or another document already says.
- Do not document deletions.
- Do not document changes over time. The history is in git.
- Do not include links to code, PRs, issues, or error pages.
- Do not explain why you did not use an alternative.
