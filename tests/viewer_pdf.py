#!/usr/bin/env python3
"""Exercise Export PDF and its native save dialog in a disposable dummy window.

Linux only; requires PyAT-SPI, an accessibility bus and a graphical session.
Run after cargo build: /usr/bin/python3 tests/viewer_pdf.py
Only the spawned test process is inspected or controlled.
"""
import argparse
from collections import deque
import os
import re
from pathlib import Path
import subprocess
import tempfile
import time

import pyatspi

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, default=Path("target/debug/mdr"))
args = parser.parse_args()


def find(root, predicate):
    queue = deque([root])
    while queue:
        node = queue.popleft()
        try:
            if predicate(node):
                return node
            queue.extend(node.getChildAtIndex(i) for i in range(node.childCount))
        except Exception:
            continue
    return None


def wait_for(predicate, message, timeout=25):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(message)


def click(node):
    assert node.queryAction().doAction(0), "Could not activate test control"


with tempfile.TemporaryDirectory(prefix="mdr-viewer-pdf-") as temporary:
    directory = Path(temporary)
    source = directory / "document.md"
    source.write_text(Path("tests/pdf.md").read_text())
    output = directory / "document.pdf"
    with (directory / "viewer.log").open("w+") as log:
        env = dict(os.environ, NO_AT_BRIDGE="0")
        process = subprocess.Popen([str(args.binary.resolve()), "--backend", "webview", "--offline", str(source)],
                                   env=env, stdout=log, stderr=log)
        try:
            desktop = pyatspi.Registry.getDesktop(0)

            def test_application():
                for index in range(desktop.childCount):
                    app = desktop.getChildAtIndex(index)
                    if app.get_process_id() == process.pid:
                        return app
                return None

            app = wait_for(test_application, "Test viewer did not register on the accessibility bus")

            def export_button():
                return find(app, lambda node: node.getRoleName() in ("button", "push button") and node.name == "Export PDF…")

            button = wait_for(export_button, "Export PDF button not found")
            click(button)
            dialog = wait_for(lambda: find(app, lambda node: node.name == "Export PDF" and
                                          node.getRoleName() in ("file chooser", "dialog", "frame")),
                              "Export did not open the native save dialog")
            save = wait_for(lambda: find(dialog, lambda node: node.name == "Save" and node.getRoleName() in ("button", "push button")),
                            "Save button not found")
            click(save)
            wait_for(lambda: output.exists(), "Viewer did not produce the selected PDF", timeout=100)
            pdf = output.read_bytes()
            assert pdf.startswith(b"%PDF-")
            assert b"/Outlines" in pdf, "Viewer export lost PDF bookmarks"
            assert b"/Subtype /Link" in pdf, "Viewer export lost contents links"
            text = subprocess.check_output(["pdftotext", str(output), "-"], text=True)
            assert "Table of Contents" in text and "DOCUMENT_END" in text
            destinations = subprocess.check_output(["pdfinfo", "-dests", str(output)], text=True)
            first_page = re.search(r'^\s*(\d+)\s+.*"pdf-export-check"', destinations, flags=re.M)
            layout = subprocess.check_output(["pdftotext", "-layout", str(output), "-"], text=True)
            assert first_page and re.search(r'PDF export check\s+' + first_page[1] + r'(?:\s|$)', layout), "Viewer contents lacks the correct printed page number"
            wait_for(lambda: button.getState().contains(pyatspi.STATE_ENABLED), "Export button stayed disabled")
            print("Native Export PDF action, save dialog, numbered contents and bookmarks: passed", flush=True)

            click(button)
            dialog = wait_for(lambda: find(app, lambda node: node.name == "Export PDF" and
                                          node.getRoleName() in ("file chooser", "dialog", "frame")), "Second save dialog not found")
            cancel = wait_for(lambda: find(dialog, lambda node: node.name == "Cancel" and node.getRoleName() in ("button", "push button")),
                              "Cancel button not found")
            click(cancel)
            wait_for(lambda: button.getState().contains(pyatspi.STATE_ENABLED), "Cancelling did not restore export control")
            assert output.read_bytes() == pdf, "Cancelling changed the PDF"
            print("Cancelling export preserves the previous PDF: passed", flush=True)
        except Exception:
            log.seek(0)
            print(log.read(), flush=True)
            if 'app' in locals():
                queue = deque([app])
                for _ in range(100):
                    if not queue:
                        break
                    node = queue.popleft()
                    try:
                        print('Test UI:', node.getRoleName(), repr(node.name), flush=True)
                        queue.extend(node.getChildAtIndex(i) for i in range(node.childCount))
                    except Exception:
                        pass
            raise
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
