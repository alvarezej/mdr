#!/usr/bin/env python3
"""Validate physical PDF layout using dummy data, Chrome and Poppler utilities.

Run after cargo build: /usr/bin/python3 tests/pdf_layout.py
Tests portrait, landscape, no-TOC output and an intentionally hostile screen theme.
"""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, default=Path("target/debug/mdr"))
parser.add_argument("--output", type=Path, help="keep a copy of the portrait sample PDF")
args = parser.parse_args()
source = Path(__file__).with_name("pdf-stress.md").resolve()
heading_count = len(re.findall(r"^#{1,6} ", source.read_text(), flags=re.M))

with tempfile.TemporaryDirectory(prefix="mdr-pdf-layout-") as temporary:
    directory = Path(temporary)
    home = directory / "home"
    style = home / ".config/mdr/style.css"
    style.parent.mkdir(parents=True)
    style.write_text("""html {font-size:72px !important} body,.content {font-size:48pt !important}
        .content {padding:200px !important} .print-toc {display:none !important}
        table {width:5000px !important} pre {white-space:nowrap !important}""")
    env = dict(os.environ, HOME=str(home))
    for variant, flags in [("portrait", []), ("landscape", ["--landscape"]), ("without-contents", ["--no-toc"])]:
        output = directory / (variant + ".pdf")
        subprocess.run([str(args.binary.resolve()), str(source), "--offline", "--export-pdf", str(output), *flags],
                       env=env, check=True, capture_output=True)
        data = output.read_bytes()
        assert data.startswith(b"%PDF-") and b"/Outlines" in data, "PDF bookmarks missing"
        outline = re.search(rb'/Type /Outlines\b([^>]*)>>', data, flags=re.S)
        count = re.search(rb'/Count\s+(\d+)', outline[1]) if outline else None
        assert count and int(count[1]) == heading_count, "Not every heading has a PDF bookmark"
        # Raw drawing order keeps each table cell together; visual layout order
        # can interleave adjacent columns between two lines of the same cell.
        text = subprocess.check_output(["pdftotext", "-raw", str(output), "-"], text=True)
        for marker in ["TABLE_END", "CODE_END", "STRESS_DOCUMENT_END"]:
            assert marker in text, "Missing text: " + marker
        compact = ''.join(text.split())
        for number in range(1, 46):
            assert f"CODE_ROW_{number:02}" in compact, f"Code row {number} was clipped"
        normalized = ' '.join(text.split())
        for number in range(1, 41):
            assert f"sample item {number}," in normalized, f"Table row {number} was clipped"
        assert ("Table of Contents" in text) == (variant != "without-contents")

        document = ET.fromstring(subprocess.check_output([
            "pdftohtml", "-xml", "-i", "-nomerge", "-zoom", "1.0", "-stdout", str(output)], stderr=subprocess.DEVNULL))
        fonts = {float(font.attrib["size"]) for font in document.findall(".//fontspec")}
        assert 10 in fonts and max(fonts) <= 20.5, f"Unexpected text scaling: {fonts}"

        bounds = ET.fromstring(subprocess.check_output(["pdftotext", "-bbox-layout", str(output), "-"]))
        ns = {"x": "http://www.w3.org/1999/xhtml"}
        pages = bounds.findall(".//x:page", ns)
        for index, page in enumerate(pages, 1):
            width, height = float(page.attrib["width"]), float(page.attrib["height"])
            assert (width > height) == (variant == "landscape")
            for word in page.findall(".//x:word", ns):
                assert float(word.attrib["xMin"]) >= 48, (index, word.text, "left overflow")
                assert float(word.attrib["xMax"]) <= width - 48, (index, word.text, "right overflow")
                assert 0 <= float(word.attrib["yMin"]) <= float(word.attrib["yMax"]) <= height, (index, word.text)

        if variant != "without-contents":
            destinations = subprocess.check_output(["pdfinfo", "-dests", str(output)], text=True)
            assert len(destinations.strip().splitlines()) - 1 == heading_count, "Contents destinations missing"
            assert b"/Subtype /Link" in data
            first_content_page = min(int(line.split()[0]) for line in destinations.strip().splitlines()[1:])
            printed_numbers = []
            for page in document.findall('page'):
                if int(page.attrib['number']) >= first_content_page:
                    continue
                for block in page.findall('text'):
                    if float(block.attrib['left']) < float(page.attrib['width']) - 95:
                        continue
                    for link in block.findall('.//a'):
                        label = ''.join(link.itertext()).strip()
                        if not label.isdecimal():
                            continue
                        target = re.search(r'#(\d+)$', link.attrib['href'])
                        assert target and int(label) == int(target[1]), f"Contents page number {label} points to {link.attrib['href']}"
                        printed_numbers.append(int(label))
            assert len(printed_numbers) == heading_count, "Some contents entries have no printed page number"
        print(f"{variant}: {len(pages)} pages, text inside margins, body 10pt, bookmarks and numbered contents verified", flush=True)
        if variant == "portrait" and args.output:
            shutil.copy2(output, args.output)
