#!/usr/bin/env python3
"""Check print readiness, TOC destinations and actual Linux viewer PDF output.

Requires PyGObject, WebKit2 4.1, GTK's Print to File backend and a display.
Export tests/pdf.md using export_mermaid_test_page first, then run:
  /usr/bin/python3 tests/webkit_pdf.py /tmp/mdr-pdf.html /tmp/mdr-webkit.pdf
"""
import argparse
import json
import pathlib
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("page", type=pathlib.Path)
parser.add_argument("output", type=pathlib.Path)
parser.add_argument("--dark", action="store_true", help="exercise printing from a dark viewer")
args = parser.parse_args()
page = args.page.resolve()
output = args.output.resolve()
window = Gtk.Window()
window.set_default_size(1100, 900)
view = WebKit2.WebView()
window.add(view)
window.show_all()
failed = False
operation = None


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
            callback(webview.evaluate_javascript_finish(result).to_string())
        except Exception as error:
            finish(error)
    view.evaluate_javascript(script, -1, None, None, None, done, None)


def print_pdf():
    global operation
    settings = Gtk.PrintSettings()
    settings.set_printer("Print to File")
    settings.set("output-file-format", "pdf")
    settings.set("output-uri", output.as_uri())
    setup = Gtk.PageSetup()
    setup.set_paper_size(Gtk.PaperSize.new("iso_a4"))
    operation = WebKit2.PrintOperation.new(view)
    operation.set_print_settings(settings)
    operation.set_page_setup(setup)
    operation.connect("failed", lambda _, error: finish(error))

    def printed(_):
        if not output.exists() or not output.read_bytes().startswith(b"%PDF-"):
            finish("WebKit did not produce a PDF")
        else:
            print("WebKit PDF:", output, flush=True)
            finish()
    operation.connect("finished", printed)
    operation.print_()


def check(value):
    state = json.loads(value)
    if not state["ready"]:
        GLib.timeout_add(50, poll)
        return
    assert state["message"] == "__MDR_IPC_PRINT__", state
    assert state["links"] and not state["missing"], state
    assert state["diagrams"] >= 2 and not state["unfinished"], state
    assert not state["error"], state
    print("Print preparation:", json.dumps(state), flush=True)
    print_pdf()


def poll():
    evaluate("""JSON.stringify({
        ready: !!window.mdrPrintTestReady,
        message: window.mdrPrintTestMessage,
        links: document.querySelectorAll('.print-toc a').length,
        missing: Array.from(document.querySelectorAll('.print-toc a'))
          .filter(a => !document.getElementById(a.getAttribute('href').slice(1)))
          .map(a => a.textContent),
        diagrams: document.querySelectorAll('pre.mermaid[data-mdr-rendered]').length,
        unfinished: document.querySelectorAll('pre.mermaid:not([data-mdr-rendered])').length,
        error: window.mdrMermaidError
    })""", check)
    return False


def loaded(_, event):
    if event != WebKit2.LoadEvent.FINISHED:
        return
    # Simulate IPC, including a pending render, and press the viewer's print
    # action. The native request must wait for the render promise to settle.
    evaluate("""window.ipc = {postMessage: function(message) {
        window.mdrPrintTestMessage = message;
        window.mdrPrintTestReady = true;
    }};
    var rendering = window.mdrMermaidReady;
    window.mdrMermaidReady = new Promise(function(resolve) {
        setTimeout(function() { Promise.resolve(rendering).then(resolve); }, 150);
    });
    window.mdrActions.print();
    String(!!window.mdrPrintTestReady)""",
             lambda value: finish("print started before rendering finished") if value == "true" else poll())


view.connect("load-changed", loaded)
view.connect("script-dialog", lambda *_: (finish("Unexpected script dialog during print preparation"), True)[1])
if args.dark:
    view.load_html(page.read_text().replace("<html>", '<html data-theme="dark">', 1), page.as_uri())
else:
    view.load_uri(page.as_uri())
GLib.timeout_add_seconds(30, lambda: finish("PDF check timed out"))
Gtk.main()
window.destroy()
sys.exit(1 if failed else 0)
