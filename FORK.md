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
- Keep tables inside the printable page, with wrapping and progressively compact
  layouts for wide tables while retaining repeated headers across pages.
- Add `--export-pdf`, with a linked contents page, PDF bookmarks, page numbers,
  portrait/landscape output, and fully rendered diagrams. The command uses an
  installed Chrome/Chromium with a temporary profile; `--no-toc` omits the
  contents page and `--pdf-browser` selects the browser.
- Add an **Export PDF…** button / **Ctrl/Cmd+Shift+E** with a native save dialog,
  portrait/landscape selection and background rendering. It uses the same
  exporter as the CLI, retaining contents links, PDF bookmarks and page numbers.
- Use a dedicated PDF stylesheet with 10-point body text, compact headings,
  proportionally sized table columns and page-bounded diagrams. Viewer zoom and
  the optional screen theme do not affect PDF export. **Print…** / **Ctrl/Cmd+P**
  remains available for ordinary printing.
- Show section page numbers with dotted leaders in the contents section. The
  exporter reads actual heading destinations from a draft PDF, inserts the page
  references, and verifies that they match the final pagination before saving.
  A fixed-width number column keeps long or wrapped entries aligned across pages.
- Keep contents links aligned with headings containing HTML entities, inline
  code, Unicode, repeated titles, or line breaks.

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

The Rust suite includes an ignored page-export helper. Actual layout regression
checks run against WebKitGTK and cover natural size, centering,
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

## Inline document styles

The webview and PDF renderer support per-occurrence `` `term`{.style} `` and
`[Markdown phrase]{.style}` annotations. Use muted built-in colour names, or
define styles under `mdr.styles` in YAML front matter, with a named or hex
`color`, optional `dark_color`, and `text`, `tint` or `underline` treatment.
Styles adapt to the viewer's theme; PDF colours remain consistent with light
mode. Heading labels, anchors and numbered contents exclude the annotation.
See [the usage guide](README.md#inline-colours-and-emphasis) and the synthetic
[demo document](tests/inline-styles.md).

## Verify PDF export

```bash
# Requires Chrome/Chromium; MDR_TEST_PDF_BROWSER can select another executable.
cargo test --locked --test pdf_export -- --include-ignored
mdr --offline tests/pdf.md --export-pdf /tmp/mdr-check.pdf

# Exercise the Linux viewer's print preparation and actual WebKit PDF output.
MDR_TEST_MARKDOWN=tests/pdf.md MDR_TEST_HTML=/tmp/mdr-pdf.html cargo test export_mermaid_test_page -- --ignored
/usr/bin/python3 tests/webkit_pdf.py /tmp/mdr-pdf.html /tmp/mdr-webkit.pdf
/usr/bin/python3 tests/webkit_pdf.py /tmp/mdr-pdf.html /tmp/mdr-webkit-dark.pdf --dark
```

The fixture covers diagrams and chart legends, a wide table, wrapped code,
duplicate headings, and special characters in contents labels. Command-line
tests also verify that a failed export preserves an existing output file.

The synthetic stress fixture contains 12-column tables, long links and code,
tall and wide diagrams, repeated headings and a longer contents page. These
checks need Poppler (`pdftotext`, `pdftohtml`, `pdfinfo`) and Chrome/Chromium:

```bash
cargo build --locked
/usr/bin/python3 tests/pdf_layout.py --output /tmp/mdr-layout-check.pdf
# Native save-file workflow; needs PyAT-SPI and a graphical session.
/usr/bin/python3 tests/viewer_pdf.py
```

The layout check tests portrait, landscape and omitted-contents output against
actual PDF text bounds and font sizes, and compares every printed contents number
with its PDF link destination. It also applies an oversized screen theme
in a disposable home directory to check that export ignores it. The viewer check
creates and controls only a temporary dummy-document window and verifies saving,
cancelling, contents links and PDF bookmarks.
