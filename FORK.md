# Personal fork

This fork keeps the original project's history and a customized WebKit viewer.

| Name | Purpose |
| --- | --- |
| `origin` | Your fork: `https://github.com/alvarezej/mdr.git` |
| `upstream` | Original project: `https://github.com/CleverCloud/mdr.git` |
| `main` | Clean copy of upstream's `main` |
| `personal` | Customized viewer; make your own changes here |

A fork is a separate repository with shared history. Committing records a change
locally; pushing uploads it to your fork. Neither operation changes CleverCloud's
repository. Sharing a change back with CleverCloud would require a pull request.

## Customizations

- Render webview Mermaid blocks with the bundled, offline Mermaid.js engine,
  retaining multiline source labels and support for XY charts.
- Center diagrams at their natural maximum width and shrink them in narrow
  windows without WebKit's zero-size SVG collapse.
- Use the document font for diagram text and include overflowing labels in the
  SVG viewport.
- Render diagrams after live reload or opening another document, including when
  the first document had no diagrams.
- Add fullscreen controls after rendering so they do not disturb temporary SVGs
  used for text measurement. Fullscreen diagrams can grow beyond their normal
  maximum width.
- Add legends for named XY-chart lines and bars, matching actual plot colors and
  retaining the legends in fullscreen and print output.
- Use a wider document layout and support `~/.config/mdr/style.css`. The optional
  customized stylesheet is saved as [`assets/editorial.css`](assets/editorial.css).

These Mermaid changes apply to the webview backend. The native egui and terminal
rendering paths still use their existing renderer.

## Install this version

For a fresh checkout:

```bash
git clone --branch personal https://github.com/alvarezej/mdr.git
cd mdr
git remote add upstream https://github.com/CleverCloud/mdr.git
cargo install --path . --locked
mdr --backend webview notes.md
```

To use the optional stylesheet, copy `assets/editorial.css` to
`~/.config/mdr/style.css` after backing up any existing stylesheet. Set
`backend webview` in `~/.config/mdr/config.kdl` to use this backend by default.
The stylesheet is read when a viewer window opens.

## Save your next customization

Work on `personal`, stage the files you changed, then commit and push:

```bash
git switch personal
git status
git add <files-you-changed>
git commit -m "Describe the change"
git push origin personal
```

## Bring in updates from the original project

Start with a clean working tree: commit any unfinished changes first.

```bash
git fetch upstream
git switch main
git merge --ff-only upstream/main
git push origin main

git switch personal
git merge main
cargo test --locked
git push origin personal
cargo install --path . --locked
```

This advances `main` to upstream's latest commit, then merges those updates into
your customized branch. Your commits remain in `personal`. This is a manual
update workflow, not automatic synchronization.

If Git reports conflicts, resolve the marked files and complete the merge before
running the remaining commands. If `--ff-only` fails, stop and inspect the branch
history; do not force-push or reset away your changes. Using merges on the shared
`personal` branch avoids rewriting its published history.

On GitHub, select `personal` to see the customized code. The default `main` branch
is intentionally the clean upstream copy.

## Verify Mermaid rendering

The Rust suite has 259 passing tests and an ignored page-export helper. Actual
layout regression checks run against WebKitGTK and cover natural size, centering,
multiline labels, named series, matching legend colors, fullscreen, and reloads.

```bash
cargo test --locked
cargo fmt --check
MDR_TEST_MARKDOWN=tests/mermaid.md MDR_TEST_HTML=/tmp/mdr-mermaid.html cargo test export_mermaid_test_page -- --ignored
/usr/bin/python3 tests/webkit_mermaid.py /tmp/mdr-mermaid.html
/usr/bin/python3 tests/webkit_mermaid.py /tmp/mdr-mermaid.html --without-custom-css --defer
```

The rendering checks require a graphical session, PyGObject, and WebKit2 4.1.
They have been exercised with WebKitGTK 2.52 on Linux. Other platform webviews
have not been checked with this regression harness.
