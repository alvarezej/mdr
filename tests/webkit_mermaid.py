#!/usr/bin/env python3
"""Exercise an exported mdr page in WebKitGTK, the actual Linux viewer engine.

Requires system Python with PyGObject/WebKit2 4.1 and a graphical session.
From the mdr checkout, export the fixture with:
  MDR_TEST_MARKDOWN=tests/mermaid.md MDR_TEST_HTML=/tmp/mdr-mermaid.html \
    cargo test export_mermaid_test_page -- --ignored
Run: /usr/bin/python3 tests/webkit_mermaid.py /absolute/path/page.html
Also run with --without-custom-css --defer to check the built-in theme and
diagrams introduced after the first render. --snapshot saves a full-page PNG.
"""
import argparse
import json
import sys
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("page")
parser.add_argument("--without-custom-css", action="store_true")
parser.add_argument("--defer", action="store_true", help="start without diagrams, then load them")
parser.add_argument("--snapshot", help="save a PNG of the wide page")
args = parser.parse_args()

window = Gtk.Window()
window.set_default_size(1400, 1000)
view = WebKit2.WebView()
window.add(view)
window.show_all()
failed = False
steps = []

MEASURE = r"""
JSON.stringify(Array.from(document.querySelectorAll('pre.mermaid')).map((pre, index) => {
  const svg = pre.querySelector('svg');
  if (!svg) return {error: pre.textContent};
  const rect = svg.getBoundingClientRect();
  const container = pre.getBoundingClientRect();
  return {width: rect.width, height: rect.height,
    available: container.width, natural: parseFloat(svg.style.maxWidth),
    centered: Math.abs(rect.x + rect.width / 2 - container.x - container.width / 2) < 2,
    lineBreaks: svg.querySelectorAll('foreignObject br').length,
    expectedBreak: /<br\s*\/?\s*>/i.test(window.originalSources[index]),
    hasStyle: !!svg.querySelector('style'),
    legend: Array.from(svg.querySelectorAll('.mdr-plot-legend > g')).map(group => {
      const index = group.dataset.seriesIndex;
      const plot = svg.querySelector(`.line-plot-${index} path, .bar-plot-${index} rect`);
      const swatch = group.querySelector('line, rect');
      const property = swatch.tagName === 'line' ? 'stroke' : 'fill';
      return {name: Array.from(group.querySelectorAll('tspan')).map(el => el.textContent).join(' '),
        colorMatches: getComputedStyle(swatch)[property] === getComputedStyle(plot)[property]};
    }),
    expectedNames: Array.from(window.originalSources[index].matchAll(/^\s*(?:line|bar)\s*"([^"\n]+)"\s*\[/gm), match => match[1]),
    label: Array.from(svg.querySelectorAll('text, foreignObject')).map(el => el.textContent).join(' '),
    clippedText: Array.from(svg.querySelectorAll('text')).filter(el => {
      const text = el.getBoundingClientRect();
      return text.width && (text.left < rect.left - 2 || text.right > rect.right + 2);
    }).map(el => el.textContent),
    labelBoxes: Array.from(svg.querySelectorAll('foreignObject')).map(el => {
      const inner = el.firstElementChild.getBoundingClientRect();
      const outer = el.getBoundingClientRect();
      return {width: outer.width, height: outer.height,
        innerWidth: inner.width, innerHeight: inner.height};
    })};
}))
"""


def finish(error=None):
    global failed
    if error:
        failed = True
        print("FAIL:", error, flush=True)
    Gtk.main_quit()
    return False


def evaluate(script, callback):
    def done(webview, result, _):
        try:
            value = webview.evaluate_javascript_finish(result)
            callback(value.to_string())
        except Exception as error:
            finish(error)
    view.evaluate_javascript(script, -1, None, None, None, done, None)


def measure(name):
    def checked(value):
        diagrams = json.loads(value)
        assert diagrams, "no Mermaid diagrams found"
        for diagram in diagrams:
            assert "error" not in diagram, diagram
            assert diagram["width"] > 20 and diagram["height"] > 20, diagram
            assert diagram["width"] <= diagram["available"] + 2, diagram
            assert abs(diagram["width"] - min(diagram["natural"], diagram["available"])) < 2, diagram
            assert diagram["centered"], diagram
            assert not diagram["clippedText"], diagram["clippedText"]
            assert diagram["hasStyle"], "zoom wrapping must preserve Mermaid's SVG stylesheet"
            assert [item["name"] for item in diagram["legend"]] == diagram["expectedNames"], diagram
            assert all(item["colorMatches"] for item in diagram["legend"]), diagram
            if diagram["expectedBreak"]:
                assert diagram["lineBreaks"] > 0, "multiline label lost its line break"
            for label in diagram["labelBoxes"]:
                assert label["innerWidth"] <= label["width"] + 2, label
                assert label["innerHeight"] <= label["height"] + 2, label
        print(name, json.dumps(diagrams), flush=True)
        next_step()
    evaluate(MEASURE, checked)


def delay(action):
    GLib.timeout_add(1200, lambda: (action(), False)[1])


def next_step():
    if not steps:
        finish()
    else:
        steps.pop(0)()


def narrow():
    evaluate("document.querySelector('.content').style.cssText += ';width:320px;max-width:320px !important;padding:0;margin:0;flex:none'; 'ok'",
             lambda _: delay(lambda: measure("narrow")))


def reload():
    # Capture and restore the original unprocessed source, as live reload does.
    evaluate("document.querySelector('.content').innerHTML = window.originalBody; window.mdrRenderMermaid(); 'ok'",
             lambda _: delay(lambda: measure("reload")))


def fullscreen():
    def check(value):
        result = json.loads(value)
        assert result["visible"] and result["width"] > 1000, result
        assert result["legend"] == result["expectedLegend"], result
        print("fullscreen", result, flush=True)
        evaluate("document.dispatchEvent(new KeyboardEvent('keydown', {key:'Escape'})); 'ok'", lambda _: next_step())
    evaluate("""document.querySelector('.expand-btn').click();
      JSON.stringify({visible: getComputedStyle(document.querySelector('#expand-overlay')).display === 'flex',
        width: document.querySelector('#expand-content > svg').getBoundingClientRect().width,
        legend: document.querySelector('#expand-content .mdr-plot-legend')?.textContent || '',
        expectedLegend: document.querySelector('.content .expandable > svg').querySelector('.mdr-plot-legend')?.textContent || ''})""", check)


def loaded(webview, event):
    if event == WebKit2.LoadEvent.FINISHED:
        if args.defer:
            steps.append(reload)
        steps.append(lambda: measure("wide"))
        if args.snapshot:
            steps.append(snapshot)
        steps.extend([fullscreen, narrow, reload])
        delay(next_step)


def snapshot():
    def done(webview, result, _):
        try:
            surface = webview.get_snapshot_finish(result)
            surface.write_to_png(args.snapshot)
            next_step()
        except Exception as error:
            finish(error)
    view.get_snapshot(WebKit2.SnapshotRegion.FULL_DOCUMENT,
                      WebKit2.SnapshotOptions.NONE, None, done, None)


# Runs before page scripts so reload tests use real original source.
manager = view.get_user_content_manager()
setup = "window.originalBody = document.querySelector('.content').innerHTML;"
setup += "window.originalSources = Array.from(document.querySelectorAll('pre.mermaid')).map(el => el.textContent);"
if args.defer:
    setup += "document.querySelector('.content').innerHTML = '<p>No diagrams yet</p>';"
if args.without_custom_css:
    setup += "Array.from(document.head.querySelectorAll('style')).pop().remove();"
manager.add_script(WebKit2.UserScript.new(
    "document.addEventListener('DOMContentLoaded', () => {" + setup + "}, {once:true});",
    WebKit2.UserContentInjectedFrames.TOP_FRAME,
    WebKit2.UserScriptInjectionTime.START, None, None))
view.connect("load-changed", loaded)
view.load_uri(GLib.filename_to_uri(args.page, None))
GLib.timeout_add_seconds(30, lambda: finish("rendering timed out"))
Gtk.main()
window.destroy()
sys.exit(1 if failed else 0)
