//! Failure paths must not destroy an earlier export or open an interactive UI.
use std::process::Command;

#[cfg(feature = "webview-backend")]
#[test]
fn unavailable_browser_leaves_existing_output_intact() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("report.md");
    let output = dir.path().join("report.pdf");
    std::fs::write(&input, "# Report\n\nSome text.").unwrap();
    std::fs::write(&output, b"previous export").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_mdr"))
        .arg(&input)
        .arg("--export-pdf")
        .arg(&output)
        .arg("--pdf-browser")
        .arg(dir.path().join("missing-browser"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Cannot start PDF browser"));
    assert_eq!(std::fs::read(&output).unwrap(), b"previous export");
}

#[test]
fn pdf_only_options_require_export() {
    let result = Command::new(env!("CARGO_BIN_EXE_mdr"))
        .args(["--landscape", "--list-backends"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--export-pdf"));
}

#[cfg(feature = "webview-backend")]
#[test]
#[ignore = "requires Chrome/Chromium; set MDR_TEST_PDF_BROWSER to override detection"]
fn real_pdf_has_a_contents_page_links_and_bookmarks() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("report with spaces.pdf");
    let mut command = Command::new(env!("CARGO_BIN_EXE_mdr"));
    command
        .arg("tests/pdf.md")
        .arg("--offline")
        .arg("--export-pdf")
        .arg(&output);
    if let Some(browser) = std::env::var_os("MDR_TEST_PDF_BROWSER") {
        command.arg("--pdf-browser").arg(browser);
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let pdf = String::from_utf8_lossy(&bytes);
    assert!(pdf.contains("/Outlines"), "PDF bookmarks are missing");
    assert!(pdf.contains("/Subtype /Link"), "contents links are missing");
}
