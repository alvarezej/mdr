#[cfg(feature = "webview-backend")]
use std::sync::OnceLock;

/// Rendering font for the graphical backends (`egui` and `webview`) and PDF export.
///
/// Built-in fonts are embedded in the binary so they work offline without
/// system installation. Any other valid name selects an installed system font
/// (falling back to the default system stack if unavailable).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Font {
    /// Space Grotesk variable font (bundled, default).
    #[default]
    SpaceGrotesk,
    /// Google Sans variable font (bundled).
    GoogleSans,
    /// Original OS / stylesheet default font stack.
    System,
    /// Arbitrary installed system font family name (e.g. `"Inter"`, `"Georgia"`).
    Custom(String),
}

#[cfg(feature = "webview-backend")]
pub const SPACE_GROTESK_TTF: &[u8] = include_bytes!("../../assets/fonts/SpaceGrotesk.ttf");

/// Static 400-weight instance for egui: the variable Space Grotesk font's
/// default `wght` axis coordinate is 300 (Light), which ab_glyph renders thin.
#[cfg(feature = "egui-backend")]
pub const SPACE_GROTESK_REGULAR_TTF: &[u8] =
    include_bytes!("../../assets/fonts/SpaceGrotesk-Regular.ttf");

#[cfg(any(feature = "egui-backend", feature = "webview-backend"))]
pub const GOOGLE_SANS_TTF: &[u8] = include_bytes!("../../assets/fonts/GoogleSans.ttf");

#[cfg(feature = "webview-backend")]
const SYSTEM_FONT_STACK: &str = r#"system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif"#;

impl Font {
    /// Parse a CLI or config font specification.
    ///
    /// Recognises `space-grotesk`, `google-sans`, and `system` (case-insensitive,
    /// ignoring hyphens, underscores, and spaces). Any other non-empty font name
    /// without control or CSS-delimiter characters is accepted as a custom system
    /// font family.
    pub fn parse(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("font name cannot be empty".to_string());
        }
        if trimmed
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | '{' | '}' | ';'))
        {
            return Err(format!("invalid character in font name '{}'", s));
        }

        let normalized: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .map(|c| c.to_ascii_lowercase())
            .collect();

        match normalized.as_str() {
            "spacegrotesk" => Ok(Self::SpaceGrotesk),
            "googlesans" => Ok(Self::GoogleSans),
            "system" => Ok(Self::System),
            _ => Ok(Self::Custom(trimmed.to_string())),
        }
    }

    /// Return the embedded TrueType bytes for built-in fonts.
    #[cfg(feature = "egui-backend")]
    pub fn embedded_ttf(&self) -> Option<&'static [u8]> {
        match self {
            Self::SpaceGrotesk => Some(SPACE_GROTESK_REGULAR_TTF),
            Self::GoogleSans => Some(GOOGLE_SANS_TTF),
            Self::System | Self::Custom(_) => None,
        }
    }

    /// Generate CSS `@font-face` and `--mdr-font-family` / `body` rules for the
    /// webview and PDF export pipelines. Returns an empty string for `Font::System`
    /// so the default or user stylesheet takes effect untouched.
    #[cfg(feature = "webview-backend")]
    pub fn webview_css(&self) -> String {
        match self {
            Self::SpaceGrotesk => {
                static CSS: OnceLock<String> = OnceLock::new();
                CSS.get_or_init(|| {
                    embedded_webview_css("Space Grotesk", "300 700", SPACE_GROTESK_TTF)
                })
                .clone()
            }
            Self::GoogleSans => {
                static CSS: OnceLock<String> = OnceLock::new();
                CSS.get_or_init(|| embedded_webview_css("Google Sans", "400 700", GOOGLE_SANS_TTF))
                    .clone()
            }
            Self::System => String::new(),
            Self::Custom(name) => {
                let escaped = escape_css_string(name);
                format!(
                    ":root {{ --mdr-font-family: \"{escaped}\", {SYSTEM_FONT_STACK}; }}\n\
                     body {{ font-family: var(--mdr-font-family); }}"
                )
            }
        }
    }
}

#[cfg(feature = "webview-backend")]
fn embedded_webview_css(family: &str, weight_range: &str, ttf_bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(ttf_bytes);
    format!(
        "@font-face {{\n\
         \tfont-family: \"{family}\";\n\
         \tsrc: url(\"data:font/ttf;base64,{b64}\") format(\"truetype\");\n\
         \tfont-weight: {weight_range};\n\
         \tfont-style: normal;\n\
         \tfont-display: swap;\n\
         }}\n\
         :root {{ --mdr-font-family: \"{family}\", {SYSTEM_FONT_STACK}; }}\n\
         body {{ font-family: var(--mdr-font-family); }}"
    )
}

#[cfg(feature = "webview-backend")]
fn escape_css_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_is_space_grotesk() {
        assert_eq!(Font::default(), Font::SpaceGrotesk);
    }

    #[test]
    fn parse_recognises_builtin_fonts_and_aliases() {
        for s in [
            "space-grotesk",
            "Space Grotesk",
            "SPACE_GROTESK",
            "spacegrotesk",
        ] {
            assert_eq!(Font::parse(s).unwrap(), Font::SpaceGrotesk, "{s}");
        }
        for s in ["google-sans", "Google Sans", "GOOGLE_SANS", "googlesans"] {
            assert_eq!(Font::parse(s).unwrap(), Font::GoogleSans, "{s}");
        }
        for s in ["system", "SYSTEM", " system "] {
            assert_eq!(Font::parse(s).unwrap(), Font::System, "{s}");
        }
    }

    #[test]
    fn parse_accepts_custom_system_font_names() {
        assert_eq!(Font::parse("Inter").unwrap(), Font::Custom("Inter".into()));
        assert_eq!(
            Font::parse("  IBM Plex Sans  ").unwrap(),
            Font::Custom("IBM Plex Sans".into())
        );
    }

    #[test]
    fn parse_rejects_empty_and_unsafe_names() {
        assert!(Font::parse("").is_err());
        assert!(Font::parse("   ").is_err());
        assert!(Font::parse("Arial</style><script>").is_err());
        assert!(Font::parse("Arial; color: red").is_err());
        assert!(Font::parse("Arial\nbody{}").is_err());
    }

    #[cfg(feature = "webview-backend")]
    #[test]
    fn embedded_webview_ttfs_have_valid_truetype_headers() {
        assert!(SPACE_GROTESK_TTF.starts_with(&[0x00, 0x01, 0x00, 0x00]));
        assert!(GOOGLE_SANS_TTF.starts_with(&[0x00, 0x01, 0x00, 0x00]));
    }

    #[cfg(feature = "egui-backend")]
    #[test]
    fn embedded_egui_ttfs_have_valid_truetype_headers() {
        assert!(SPACE_GROTESK_REGULAR_TTF.starts_with(&[0x00, 0x01, 0x00, 0x00]));
        assert!(GOOGLE_SANS_TTF.starts_with(&[0x00, 0x01, 0x00, 0x00]));
    }

    #[cfg(feature = "webview-backend")]
    #[test]
    fn webview_css_embeds_data_uri_for_builtin_fonts() {
        let sg = Font::SpaceGrotesk.webview_css();
        assert!(sg.contains("@font-face"));
        assert!(sg.contains(r#"font-family: "Space Grotesk""#));
        assert!(sg.contains("data:font/ttf;base64,"));
        assert!(sg.contains("--mdr-font-family:"));

        let gs = Font::GoogleSans.webview_css();
        assert!(gs.contains("@font-face"));
        assert!(gs.contains(r#"font-family: "Google Sans""#));
        assert!(gs.contains("data:font/ttf;base64,"));

        assert!(Font::System.webview_css().is_empty());

        let custom = Font::Custom("IBM Plex \"Sans\"".into()).webview_css();
        assert!(!custom.contains("@font-face"));
        assert!(custom.contains(r#"--mdr-font-family: "IBM Plex \"Sans\"""#));
    }
}
