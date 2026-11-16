//! PDF export reuses the viewer's content pipeline with an independent page layout.
//! Chrome runs in an isolated temporary profile.
//! Render to a frozen HTML document first, checking an explicit readiness marker;
//! printing that document cannot race Mermaid's asynchronous rendering.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::backend::webview::pdf_html;
use crate::core::toc::{extract_toc, TocEntry};

const PDF_CSS: &str = include_str!("../../assets/pdf.css");
const PDF_JS: &str = include_str!("../../assets/pdf.js");

pub struct PdfOptions {
    pub toc: bool,
    pub landscape: bool,
    pub browser: Option<PathBuf>,
}

pub fn export(
    input: &Path,
    output: &Path,
    options: &PdfOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    if !output
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        return Err("PDF output must have a .pdf extension".into());
    }
    let input = input.canonicalize()?;
    if output.canonicalize().ok().as_ref() == Some(&input) {
        return Err("PDF output must differ from the Markdown input".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Create the destination's temporary file now, so bad paths/permissions fail
    // before doing any rendering. Persist only after a successful PDF export.
    let mut destination = tempfile::NamedTempFile::new_in(parent)?;
    let browser = find_browser(options.browser.as_deref())?;
    let work = tempfile::Builder::new().prefix("mdr-pdf-").tempdir()?;
    let page = work.path().join("document.html");
    let rendered = work.path().join("rendered.html");
    let pdf = work.path().join("document.pdf");
    let source = std::fs::read_to_string(&input)?;
    let html = export_html(&source, input.parent().unwrap_or(Path::new(".")), options);
    std::fs::write(&page, html)?;

    run_browser(
        &browser,
        work.path(),
        &[
            OsString::from("--dump-dom"),
            OsString::from("--virtual-time-budget=30000"),
            OsString::from(file_url(&page)),
        ],
        &rendered,
    )?;
    let html = std::fs::read_to_string(&rendered)?;
    let mut frozen = freeze_rendered_html(&html)?;
    // Keep the same URL: Chrome's PDF link destinations must refer to this page.
    std::fs::write(&page, &frozen)?;
    print_pdf(&browser, work.path(), &page, &pdf)?;

    let entries = if options.toc {
        extract_toc(&source)
    } else {
        Vec::new()
    };
    if !entries.is_empty() {
        // Read actual PDF page destinations, not estimated DOM heights. The
        // number column is reserved before the first print, so adding numbers
        // normally takes one additional pass. Verify the final pagination and
        // refuse to publish an export whose contents numbers did not converge.
        for pass in 0..4 {
            let pages = heading_pages(&pdf, &entries)?;
            let numbered = number_toc(&frozen, &pages)?;
            if numbered == frozen {
                break;
            }
            if pass == 3 {
                return Err("PDF contents pagination did not stabilize".into());
            }
            frozen = numbered;
            std::fs::write(&page, &frozen)?;
            print_pdf(&browser, work.path(), &page, &pdf)?;
        }
    }

    let mut result = std::fs::File::open(&pdf)
        .map_err(|error| io::Error::other(format!("Browser did not produce a PDF: {error}")))?;
    let mut signature = [0; 5];
    result.read_exact(&mut signature)?;
    if &signature != b"%PDF-" {
        return Err("Browser output is not a PDF".into());
    }
    destination.write_all(&signature)?;
    io::copy(&mut result, &mut destination)?;
    destination.persist(output)?;
    Ok(())
}

fn print_pdf(browser: &Path, work: &Path, page: &Path, pdf: &Path) -> io::Result<()> {
    match std::fs::remove_file(pdf) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut print_arg = OsString::from("--print-to-pdf=");
    print_arg.push(pdf);
    run_browser(
        browser,
        work,
        &[
            print_arg,
            OsString::from("--no-pdf-header-footer"),
            OsString::from("--generate-pdf-document-outline"),
            OsString::from(file_url(page)),
        ],
        &work.join("print.log"),
    )
}

fn heading_pages(pdf: &Path, entries: &[TocEntry]) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let document = lopdf::Document::load(pdf)?;
    let destinations = destination_pages(&document)?;
    entries
        .iter()
        .map(|entry| {
            // Chrome stores non-ASCII fragment bytes as URI percent escapes
            // in PDF destination names, even though the HTML IDs are Unicode.
            destinations
                .get(entry.anchor.as_bytes())
                .or_else(|| destinations.get(encoded_fragment(&entry.anchor).as_bytes()))
                .copied()
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "PDF is missing a destination for heading '{}'",
                        entry.text
                    ))
                    .into()
                })
        })
        .collect()
}

fn encoded_fragment(anchor: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in anchor.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

fn destination_pages(
    document: &lopdf::Document,
) -> Result<BTreeMap<Vec<u8>, u32>, Box<dyn std::error::Error>> {
    let page_numbers: BTreeMap<_, _> = document
        .get_pages()
        .into_iter()
        .map(|(number, id)| (id, number))
        .collect();
    let page_number = |target: &lopdf::Object| -> Result<u32, Box<dyn std::error::Error>> {
        let (id, object) = document.dereference(target)?;
        if let Some(id) = id {
            return page_numbers.get(&id).copied().ok_or_else(|| {
                io::Error::other("PDF destination does not reference a page").into()
            });
        }
        if let lopdf::Object::Integer(index) = object {
            let number = u32::try_from(*index)?
                .checked_add(1)
                .ok_or_else(|| io::Error::other("PDF page index overflow"))?;
            if number as usize <= page_numbers.len() {
                return Ok(number);
            }
        }
        Err(io::Error::other("Invalid PDF page destination").into())
    };
    let catalog = document.catalog()?;
    let mut result = BTreeMap::new();
    // Chrome currently writes the flat PDF /Dests dictionary. Resolve both
    // array destinations and the equivalent dictionary form containing /D.
    if let Ok(flat) = document.get_dict_in_dict(catalog, b"Dests") {
        for (name, value) in flat.iter() {
            let (_, value) = document.dereference(value)?;
            let value = match value {
                lopdf::Object::Dictionary(dict) => dict.get_deref(b"D", document)?,
                value => value,
            };
            let target = value
                .as_array()?
                .first()
                .ok_or_else(|| io::Error::other("Empty PDF destination"))?;
            result.insert(name.clone(), page_number(target)?);
        }
    }
    // Also accept the standard /Names /Dests name tree, including child nodes.
    if let Ok(names) = document.get_dict_in_dict(catalog, b"Names") {
        if let Ok(tree) = document.get_dict_in_dict(names, b"Dests") {
            let mut named = Default::default();
            document.get_named_destinations(tree, &mut named)?;
            for (name, destination) in named {
                result.insert(name, page_number(destination.page()?)?);
            }
        }
    }
    Ok(result)
}

fn number_toc(html: &str, pages: &[u32]) -> io::Result<String> {
    let start = html
        .find("<nav class=\"print-toc\"")
        .ok_or_else(|| io::Error::other("PDF contents section missing"))?;
    let end = start
        + html[start..]
            .find("</nav>")
            .ok_or_else(|| io::Error::other("PDF contents section is not closed"))?;
    let spans =
        regex::Regex::new(r#"<span class="toc-page" data-mdr-toc-index="(\d+)">[^<]*</span>"#)
            .unwrap();
    let mut seen = vec![false; pages.len()];
    let mut count = 0;
    let mut invalid = false;
    let numbered = spans.replace_all(&html[start..end], |caps: &regex::Captures| {
        let index = caps[1].parse::<usize>().ok();
        match index.and_then(|i| pages.get(i).map(|number| (i, number))) {
            Some((index, number)) => {
                count += 1;
                seen[index] = true;
                format!(r#"<span class="toc-page" data-mdr-toc-index="{index}">{number}</span>"#)
            }
            None => {
                invalid = true;
                caps[0].to_string()
            }
        }
    });
    if invalid || count != pages.len() || seen.contains(&false) {
        return Err(io::Error::other(
            "PDF contents entries do not match heading destinations",
        ));
    }
    Ok(format!("{}{}{}", &html[..start], numbered, &html[end..]))
}

fn export_html(markdown: &str, base: &Path, options: &PdfOptions) -> String {
    let html = pdf_html(markdown, base).replacen(
        "<html>",
        "<html class=\"mdr-pdf\" data-theme=\"light\">",
        1,
    );
    let orientation = if options.landscape {
        "landscape"
    } else {
        "portrait"
    };
    let height = if options.landscape { "160mm" } else { "245mm" };
    let width = if options.landscape { "261mm" } else { "174mm" };
    let style = format!("<style>{PDF_CSS}\n@page {{ size: A4 {orientation}; }} :root {{ --mdr-print-height: {height}; --mdr-content-width: {width}; }}</style></head>");
    let html = html.replacen("</head>", &style, 1);
    let no_toc = if options.toc {
        ""
    } else {
        "document.body.classList.add('mdr-no-print-toc');"
    };
    let script = format!(
        r#"<script>{PDF_JS}</script><script>
{no_toc}
document.addEventListener('DOMContentLoaded', function() {{
window.mdrPreparePrint().then(function() {{
    window.mdrPreparePdf();
    document.documentElement.setAttribute('data-mdr-pdf-ready', 'true');
}}).catch(function(error) {{
    document.documentElement.setAttribute('data-mdr-pdf-error', String(error));
}});
}}, {{once: true}});
</script>"#
    );
    // Mermaid contains literal closing body tags inside its bundled JS.
    // Insert at the template's final closing tag, never inside that library.
    let mut html = html;
    let end = html.rfind("</body>").unwrap();
    html.insert_str(end, &script);
    html
}

fn freeze_rendered_html(html: &str) -> io::Result<String> {
    let root = regex::Regex::new(r"(?is)<html\b[^>]*>").unwrap();
    let root = root.find(html).map(|m| m.as_str()).unwrap_or_default();
    if !root.contains("data-mdr-pdf-ready=\"true\"") {
        let error = regex::Regex::new(r#"data-mdr-pdf-error="([^"]*)""#).unwrap();
        let detail = error
            .captures(root)
            .map(|c| c[1].to_string())
            .unwrap_or_else(|| {
                "Rendering did not finish within the browser's time budget".to_string()
            });
        return Err(io::Error::other(format!("Cannot export PDF: {detail}")));
    }
    // All SVGs, legends, highlights and data-URI images are now self contained.
    // Remove scripts so the second browser run cannot restart rendering.
    let scripts = regex::Regex::new(r"(?is)<script\b[^>]*>.*?</script\s*>").unwrap();
    Ok(scripts.replace_all(html, "").into_owned())
}

fn run_browser(browser: &Path, work: &Path, args: &[OsString], stdout: &Path) -> io::Result<()> {
    let errors = work.join("browser.log");
    let mut profile = OsString::from("--user-data-dir=");
    profile.push(work.join("profile"));
    let mut child = Command::new(browser)
        .args([
            "--headless",
            "--disable-gpu",
            "--disable-background-networking",
            "--no-first-run",
            "--no-default-browser-check",
        ])
        .arg(profile)
        .args(args)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(stdout)?)
        .stderr(std::fs::File::create(&errors)?)
        .spawn()
        .map_err(|error| {
            io::Error::other(format!(
                "Cannot start PDF browser '{}': {error}",
                browser.display()
            ))
        })?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let diagnostic = std::fs::read_to_string(&errors).unwrap_or_default();
            return Err(io::Error::other(format!(
                "PDF browser failed ({status}): {}",
                diagnostic.trim()
            )));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PDF browser timed out after 90 seconds",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn find_browser(explicit: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("MDR_PDF_BROWSER") {
        return Ok(PathBuf::from(path));
    }
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "msedge",
        "chrome.exe",
        "msedge.exe",
    ];
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for name in names {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    let mut candidates = vec![
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
    ];
    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(directory) = std::env::var_os(variable) {
            for suffix in [
                "Google/Chrome/Application/chrome.exe",
                "Microsoft/Edge/Application/msedge.exe",
            ] {
                candidates.push(PathBuf::from(&directory).join(suffix));
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound,
        "PDF export needs Chrome or Chromium. Install one, or set --pdf-browser PATH (or MDR_PDF_BROWSER). Viewer printing is also available with Ctrl/Cmd+P."))
}

fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut url = if path.starts_with('/') {
        "file://".to_string()
    } else {
        "file:///".to_string()
    };
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            url.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(&mut url, "%{byte:02X}").unwrap();
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations_use_physical_page_order_and_accept_nested_name_trees() {
        use lopdf::{dictionary, Document, Object};
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        // Allocate objects in reverse order to distinguish IDs from page numbers.
        let second = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
        let first = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
        document.objects.insert(
            pages_id,
            dictionary! {
                "Type" => "Pages", "Count" => 2,
                "Kids" => vec![Object::Reference(first), Object::Reference(second)]
            }
            .into(),
        );
        let flat = document.add_object(dictionary! {
            "setup-1" => vec![Object::Reference(second), Object::Name(b"Fit".to_vec())]
        });
        let target = document.add_object(dictionary! {
            "D" => vec![Object::Reference(first), Object::Name(b"Fit".to_vec())]
        });
        let leaf = document.add_object(dictionary! {
            "Names" => vec![Object::string_literal("café"), Object::Reference(target)]
        });
        let tree = document.add_object(dictionary! { "Kids" => vec![Object::Reference(leaf)] });
        let names = document.add_object(dictionary! { "Dests" => tree });
        let root = document.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id, "Dests" => flat, "Names" => names
        });
        document.trailer.set("Root", root);
        let pages = destination_pages(&document).unwrap();
        assert_eq!(pages.get(b"setup-1".as_slice()), Some(&2));
        assert_eq!(pages.get("café".as_bytes()), Some(&1));
    }

    #[test]
    fn contents_numbering_preserves_labels_and_body_and_is_stable() {
        let html = r##"<nav class="print-toc"><ul><li><a href="#a"><span class="toc-label">Tables &amp; code</span><span class="toc-page" data-mdr-toc-index="0">—</span></a></li><li><span class="toc-page" data-mdr-toc-index="1">—</span></li></ul></nav><main><span class="toc-page" data-mdr-toc-index="0">keep this text</span></main>"##;
        let numbered = number_toc(html, &[3, 12]).unwrap();
        assert!(numbered.contains(r#"data-mdr-toc-index="0">3</span>"#));
        assert!(numbered.contains(r#"data-mdr-toc-index="1">12</span>"#));
        assert!(numbered.contains("Tables &amp; code"));
        assert!(numbered.ends_with(
            r#"<main><span class="toc-page" data-mdr-toc-index="0">keep this text</span></main>"#
        ));
        assert_eq!(number_toc(&numbered, &[3, 12]).unwrap(), numbered);
        assert!(number_toc(html, &[3]).is_err());
    }

    #[test]
    #[ignore = "exports the dedicated PDF page for browser layout checks"]
    fn export_pdf_test_page() {
        let input = PathBuf::from(std::env::var("MDR_TEST_MARKDOWN").unwrap())
            .canonicalize()
            .unwrap();
        let source = std::fs::read_to_string(&input).unwrap();
        let options = PdfOptions {
            toc: true,
            landscape: false,
            browser: None,
        };
        let html = export_html(&source, input.parent().unwrap(), &options);
        std::fs::write(std::env::var("MDR_TEST_PDF_HTML").unwrap(), html).unwrap();
    }

    #[test]
    fn frozen_export_preserves_rendered_diagrams_and_refuses_unfinished_pages() {
        let rendered = r#"<html data-mdr-pdf-ready="true"><style>svg {width:100%}</style><svg><text>Chart</text></svg><script>renderAgain()</script></html>"#;
        let frozen = freeze_rendered_html(rendered).unwrap();
        assert!(frozen.contains("<svg><text>Chart</text></svg>"));
        assert!(!frozen.contains("<script"));
        assert!(freeze_rendered_html("<html>still rendering</html>").is_err());
        let error =
            freeze_rendered_html(r#"<html data-mdr-pdf-error="Invalid diagram">"#).unwrap_err();
        assert!(error.to_string().contains("Invalid diagram"));
    }

    #[test]
    fn file_urls_encode_spaces_fragments_and_unicode() {
        assert_eq!(
            file_url(Path::new("/tmp/Café #1%.html")),
            "file:///tmp/Caf%C3%A9%20%231%25.html"
        );
    }

    #[test]
    fn export_rejects_a_source_as_the_output_before_starting_a_browser() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("report.pdf");
        std::fs::write(&input, "# Markdown with a surprising extension").unwrap();
        let options = PdfOptions {
            toc: true,
            landscape: false,
            browser: Some(PathBuf::from("missing-browser")),
        };
        assert!(export(&input, &input, &options)
            .unwrap_err()
            .to_string()
            .contains("must differ"));
        assert_eq!(
            std::fs::read_to_string(input).unwrap(),
            "# Markdown with a surprising extension"
        );
    }
}
