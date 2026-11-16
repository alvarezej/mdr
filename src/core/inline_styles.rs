//! Per-occurrence inline styles: `code`{.name} and [Markdown text]{.name}.
//! Work on parsed inline nodes so code fences, escapes, links and heading text
//! retain their normal Markdown semantics. Only validated colours enter CSS.
use comrak::nodes::{Ast, AstNode, NodeValue};
use comrak::Arena;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

const BASE_CSS: &str = include_str!("../../assets/inline-styles.css");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgb([u8; 3]);

impl Rgb {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(hex) = value.strip_prefix('#') {
            let expanded;
            let hex = match hex.len() {
                3 => {
                    expanded = hex.chars().flat_map(|ch| [ch, ch]).collect::<String>();
                    expanded.as_str()
                }
                6 => hex,
                _ => return None,
            };
            if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            return Some(Self([
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            ]));
        }
        let hex = match value.to_ascii_lowercase().as_str() {
            "blue" => "#5B6EA6",
            "purple" => "#80658F",
            "orange" | "amber" => "#9A6728",
            "red" => "#AD5454",
            "green" => "#46734F",
            "teal" | "cyan" => "#3F7878",
            "pink" => "#A15C7A",
            "yellow" => "#847027",
            "gray" | "grey" => "#687180",
            _ => return None,
        };
        Self::parse(hex)
    }

    fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }
    fn channels(self) -> String {
        format!("{},{},{}", self.0[0], self.0[1], self.0[2])
    }

    fn luminance(self) -> f64 {
        let values = self.0.map(|value| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        });
        values[0] * 0.2126 + values[1] * 0.7152 + values[2] * 0.0722
    }

    fn dark(self) -> Self {
        let background = Self([24, 29, 39]).luminance();
        for step in 0..=50 {
            let amount = f64::from(step) / 50.0;
            let candidate =
                Self(self.0.map(|value| {
                    (f64::from(value) + f64::from(255 - value) * amount).round() as u8
                }));
            if (candidate.luminance() + 0.05) / (background + 0.05) >= 4.5 {
                return candidate;
            }
        }
        Self([255; 3])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Treatment {
    #[default]
    Text,
    Tint,
    Underline,
}

impl Treatment {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "tint" => Some(Self::Tint),
            "underline" => Some(Self::Underline),
            _ => None,
        }
    }
    fn class(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Tint => "tint",
            Self::Underline => "underline",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Style {
    light: Rgb,
    dark: Rgb,
    treatment: Treatment,
}

impl Style {
    fn from_value(value: &Value) -> Option<Self> {
        let (color, treatment, dark) = match value {
            Value::String(color) => (color.as_str(), Treatment::Text, None),
            Value::Object(fields) => (
                fields.get("color")?.as_str()?,
                fields
                    .get("treatment")
                    .map(|v| v.as_str().and_then(Treatment::parse))
                    .unwrap_or(Some(Treatment::Text))?,
                fields
                    .get("dark_color")
                    .map(|v| v.as_str().and_then(Rgb::parse)),
            ),
            _ => return None,
        };
        let light = Rgb::parse(color)?;
        Some(Self {
            light,
            dark: dark.unwrap_or_else(|| Some(light.dark()))?,
            treatment,
        })
    }
}

#[derive(Default)]
struct Styles {
    named: BTreeMap<String, Style>,
    used: Vec<Style>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

impl Styles {
    fn read<'a>(root: &'a AstNode<'a>) -> Self {
        let mut styles = Self::default();
        let front = root
            .children()
            .find_map(|node| match &node.data.borrow().value {
                NodeValue::FrontMatter(value) => Some(value.clone()),
                _ => None,
            });
        let Some(front) = front else {
            return styles;
        };
        if front.len() > 64 * 1024 || !front.contains("mdr") {
            return styles;
        }
        let front = front.trim_end();
        let Some(start) = front.find('\n').map(|index| index + 1) else {
            return styles;
        };
        let Some(end) = front.rfind('\n') else {
            return styles;
        };
        if start > end {
            return styles;
        }
        let options = serde_saphyr::options! {
            with_snippet: false,
            budget: serde_saphyr::budget! {
                max_depth: 32, max_events: 10_000, max_nodes: 5_000,
                max_total_scalar_bytes: 131_072, max_documents: 1, max_aliases: 64,
            },
        };
        let metadata: Value = match serde_saphyr::from_str_with_options(&front[start..end], options)
        {
            Ok(value) => value,
            Err(error) => {
                crate::vlog!("inline styles: invalid metadata: {}", error);
                return styles;
            }
        };
        let Some(definitions) = metadata
            .get("mdr")
            .and_then(|v| v.get("styles"))
            .and_then(Value::as_object)
        else {
            return styles;
        };
        for (name, definition) in definitions {
            if valid_name(name) {
                if let Some(style) = Style::from_value(definition) {
                    styles.named.insert(name.clone(), style);
                }
            }
        }
        styles
    }

    fn marker<'s>(&self, text: &'s str) -> Option<(&'s str, Style, usize)> {
        let rest = text.strip_prefix("{.")?;
        let end = rest.find('}')?;
        let name = &rest[..end];
        if !valid_name(name) {
            return None;
        }
        let style = self.named.get(name).copied().or_else(|| {
            let light = Rgb::parse(name)?;
            Some(Style {
                light,
                dark: light.dark(),
                treatment: Treatment::Text,
            })
        })?;
        Some((name, style, end + 3))
    }

    fn opening(&mut self, name: &str, style: Style) -> String {
        let index = match self.used.iter().position(|item| *item == style) {
            Some(index) => index,
            None => {
                self.used.push(style);
                self.used.len() - 1
            }
        };
        format!(
            r#"<span class="mdr-inline mdr-inline-{} mdr-inline-style-{index}" data-mdr-style="{name}">"#,
            style.treatment.class()
        )
    }

    fn css(&self) -> String {
        if self.used.is_empty() {
            return String::new();
        }
        let mut css = BASE_CSS.to_string();
        for (index, style) in self.used.iter().enumerate() {
            let selector = format!(".content .mdr-inline-style-{index}");
            let light = format!(
                "--mdr-inline-accent:{};--mdr-inline-tint:rgba({},0.09);",
                style.light.hex(),
                style.light.channels()
            );
            let dark = format!(
                "--mdr-inline-accent:{};--mdr-inline-tint:rgba({},0.12);",
                style.dark.hex(),
                style.dark.channels()
            );
            write!(&mut css, "{selector}{{{light}}}\n@media(prefers-color-scheme:dark){{html:not([data-theme=light]) {selector}{{{dark}}}}}\nhtml[data-theme=dark] {selector}{{{dark}}}\n@media print{{html {selector}{{--mdr-inline-accent:{}!important;--mdr-inline-tint:rgba({},0.09)!important;}}}}\n", style.light.hex(), style.light.channels()).unwrap();
        }
        format!("<style data-mdr-inline-styles>{css}</style>\n")
    }
}

fn eligible(node: &AstNode<'_>) -> bool {
    let mut current = Some(node);
    while let Some(node) = current {
        if matches!(
            node.data.borrow().value,
            NodeValue::Image(_)
                | NodeValue::CodeBlock(_)
                | NodeValue::HtmlBlock(_)
                | NodeValue::FrontMatter(_)
                | NodeValue::Escaped
        ) {
            return false;
        }
        current = node.parent();
    }
    true
}

fn text_node<'a>(arena: &'a Arena<'a>, value: String, source: &AstNode<'a>) -> &'a AstNode<'a> {
    arena.alloc(AstNode::from(Ast::new_with_sourcepos(
        NodeValue::Text(value.into()),
        source.data.borrow().sourcepos,
    )))
}

fn consume_marker(node: &AstNode<'_>, count: usize) {
    let empty = {
        let mut ast = node.data.borrow_mut();
        let NodeValue::Text(value) = &mut ast.value else {
            return;
        };
        *value = value[count..].to_string().into();
        value.is_empty()
    };
    if empty {
        node.detach();
    }
}

/// Apply known style annotations and return the document's validated stylesheet.
/// Original Code/Text nodes remain in the tree so TOC text and anchors stay clean.
pub fn apply<'a>(root: &'a AstNode<'a>, arena: &'a Arena<'a>) -> String {
    let mut styles = Styles::read(root);
    let codes: Vec<_> = root
        .descendants()
        .filter(|node| matches!(node.data.borrow().value, NodeValue::Code(_)))
        .collect();
    for node in codes {
        if !eligible(node) {
            continue;
        }
        let Some(next) = node.next_sibling() else {
            continue;
        };
        let marker = {
            let data = next.data.borrow();
            let NodeValue::Text(text) = &data.value else {
                continue;
            };
            styles
                .marker(text)
                .map(|(name, style, count)| (name.to_string(), style, count))
        };
        let Some((name, style, count)) = marker else {
            continue;
        };
        node.insert_before(arena.alloc(AstNode::from(NodeValue::HtmlInline(
            styles.opening(&name, style),
        ))));
        node.insert_after(arena.alloc(AstNode::from(NodeValue::HtmlInline("</span>".to_string()))));
        consume_marker(next, count);
    }

    let parents: Vec<_> = root
        .descendants()
        .filter(|node| node.first_child().is_some())
        .collect();
    for parent in parents {
        if !eligible(parent) {
            continue;
        }
        let children: Vec<_> = parent.children().collect();
        for node in children {
            let text = match &node.data.borrow().value {
                NodeValue::Text(text) if text.contains(['[', ']']) => text.to_string(),
                _ => continue,
            };
            let mut start = 0;
            for (index, ch) in text
                .char_indices()
                .filter(|(_, ch)| *ch == '[' || *ch == ']')
            {
                if start < index {
                    node.insert_before(text_node(arena, text[start..index].to_string(), node));
                }
                node.insert_before(text_node(arena, ch.to_string(), node));
                start = index + 1;
            }
            if start < text.len() {
                node.insert_before(text_node(arena, text[start..].to_string(), node));
            }
            node.detach();
        }
        let mut openers = Vec::new();
        for node in parent.children().collect::<Vec<_>>() {
            if node.parent().is_none() {
                continue;
            }
            let delimiter = match &node.data.borrow().value {
                NodeValue::Text(text) if text.as_ref() == "[" => '[',
                NodeValue::Text(text) if text.as_ref() == "]" => ']',
                _ => continue,
            };
            if delimiter == '[' {
                openers.push(node);
                continue;
            }
            let Some(opener) = openers.pop() else {
                continue;
            };
            let Some(next) = node.next_sibling() else {
                continue;
            };
            let marker = {
                let data = next.data.borrow();
                let NodeValue::Text(text) = &data.value else {
                    continue;
                };
                styles
                    .marker(text)
                    .map(|(name, style, count)| (name.to_string(), style, count))
            };
            let Some((name, style, count)) = marker else {
                continue;
            };
            opener.data.borrow_mut().value = NodeValue::HtmlInline(styles.opening(&name, style));
            node.data.borrow_mut().value = NodeValue::HtmlInline("</span>".to_string());
            consume_marker(next, count);
        }
    }
    styles.css()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{markdown::parse_markdown, toc::extract_toc};

    fn body(html: &str) -> &str {
        html.split("<style data-mdr-inline-styles>").next().unwrap()
    }

    #[test]
    fn styles_only_the_annotated_occurrence() {
        let html = parse_markdown("`term`{.blue}, `term`, [a new phrase]{.purple}, a new phrase.");
        assert_eq!(html.matches("data-mdr-style=").count(), 2);
        assert!(body(&html)
            .contains("data-mdr-style=\"blue\"><code>term</code></span>, <code>term</code>"));
        assert!(
            body(&html).contains("data-mdr-style=\"purple\">a new phrase</span>, a new phrase.")
        );
    }

    #[test]
    fn reads_yaml_named_styles_and_all_treatments() {
        let html = parse_markdown(
            r##"---
title: An unrelated field
mdr:
  styles:
    concept: "#5B6EA6"
    note:
      color: purple
      treatment: tint
    warning:
      color: "#abc"
      dark_color: "#DEF"
      treatment: underline
---
`term`{.concept} [a note]{.note} [pay attention]{.warning}
"##,
        );
        assert!(html.contains("mdr-inline-text mdr-inline-style-0"));
        assert!(html.contains("mdr-inline-tint mdr-inline-style-1"));
        assert!(html.contains("mdr-inline-underline mdr-inline-style-2"));
        assert!(html.contains("--mdr-inline-accent:#5B6EA6;"));
        assert!(html.contains("--mdr-inline-accent:#80658F;"));
        assert!(html.contains("--mdr-inline-accent:#AABBCC;"));
        assert!(html.contains("--mdr-inline-accent:#DDEEFF;"));
        assert!(!body(&html).contains("An unrelated field"));
    }

    #[test]
    fn preserves_nested_inline_markdown() {
        let html = parse_markdown(
            "[A **strong** phrase with `code` and [a link](https://example.com)]{.blue}.",
        );
        let html = body(&html);
        assert!(html.contains("data-mdr-style=\"blue\">A <strong>strong</strong> phrase with <code>code</code> and <a href=\"https://example.com\">a link</a></span>."), "{html}");
    }

    #[test]
    fn nests_styles_without_losing_text() {
        let html = parse_markdown("[outer [inner]{.red} and `code`{.green}]{.blue}");
        assert_eq!(html.matches("data-mdr-style=").count(), 3);
        assert!(body(&html).contains("data-mdr-style=\"blue\">outer <span"));
        assert!(body(&html).contains("data-mdr-style=\"red\">inner</span> and <span"));
        assert!(body(&html).ends_with("<code>code</code></span></span></p>\n"));
    }

    #[test]
    fn heading_text_and_duplicate_anchors_match_toc() {
        let md = "# [Café & API]{.blue}\n\n## `term`{.purple}\n\n## `term`{.purple}\n";
        let html = parse_markdown(md);
        let toc = extract_toc(md);
        assert_eq!(
            toc.iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["Café & API", "term", "term"]
        );
        assert_eq!(toc[2].anchor, "term-1");
        for entry in toc {
            assert!(html.contains(&format!("id=\"{}\"", entry.anchor)), "{html}");
        }
    }

    #[test]
    fn works_in_tables_and_multiline_phrases() {
        let html = parse_markdown("| Name | Meaning |\n|---|---|\n| `term`{.blue} | [a phrase]{.teal} |\n\n[Across\nlines]{.green}\n");
        assert_eq!(html.matches("data-mdr-style=").count(), 3);
        assert!(body(&html).contains("<table>"));
        assert!(body(&html).contains("data-mdr-style=\"green\">Across\nlines</span>"));
    }

    #[test]
    fn keeps_links_images_and_code_literal() {
        let html = parse_markdown(
            r#"[link](https://example.com/path?q=[literal]){.blue}

![alt [text]{.red}](image.png)

`[literal]{.blue}`

```text
`term`{.blue} [phrase]{.red}
```
"#,
        );
        assert!(!html.contains("data-mdr-style="), "{html}");
        assert!(html.contains("alt=\"alt [text]{.red}\""));
        assert!(html.contains("<code>[literal]{.blue}</code>"));
        assert!(html.contains("`term`{.blue} [phrase]{.red}"));
    }

    #[test]
    fn respects_escaped_brackets_and_markers() {
        let html = parse_markdown(
            r#"\[literal]{.blue} [literal\]{.blue} [literal]\{.blue} `literal`\{.blue}"#,
        );
        assert!(!html.contains("data-mdr-style="), "{html}");
        assert!(!html.contains("data-mdr-inline-styles"));
        assert!(html.contains("[literal]{.blue}"));
    }

    #[test]
    fn unknown_names_and_invalid_metadata_stay_literal() {
        let html = parse_markdown(
            "---\nmdr: [not valid yaml\n---\n\n`term`{.unknown} [phrase]{.unknown}\n",
        );
        assert_eq!(
            body(&html),
            "<p><code>term</code>{.unknown} [phrase]{.unknown}</p>\n"
        );
        assert!(!html.contains("data-mdr-inline-styles"));
        let html = parse_markdown("---\nmdr: [not valid yaml\n---\n\n[Still works]{.blue}\n");
        assert!(html.contains("data-mdr-style=\"blue\""));
    }

    #[test]
    fn rejects_invalid_colours_and_treatments_individually() {
        let html = parse_markdown(
            r##"---
mdr:
  styles:
    bad: "red; background:url(https://example.com)"
    worse:
      color: blue
      treatment: flashing
    bad_dark:
      color: blue
      dark_color: "var(--anything)"
    good: teal
---
[one]{.bad} [two]{.worse} [three]{.bad_dark} [four]{.good}
"##,
        );
        assert_eq!(html.matches("data-mdr-style=").count(), 1);
        assert!(html.contains("[one]{.bad} [two]{.worse} [three]{.bad_dark}"));
        assert!(!html.contains("background:url"));
    }

    #[test]
    fn keeps_inline_code_html_escaped() {
        let html = parse_markdown("`<script>oops</script>`{.red}");
        assert!(body(&html).contains("<code>&lt;script&gt;oops&lt;/script&gt;</code></span>"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn parses_hex_and_muted_palette_and_adapts_dark_contrast() {
        assert_eq!(Rgb::parse("#abc"), Some(Rgb([170, 187, 204])));
        for invalid in [
            "#1234",
            "#12345678",
            "#éa",
            "rgb(1,2,3)",
            "var(--color)",
            "transparent",
        ] {
            assert_eq!(Rgb::parse(invalid), None);
        }
        for name in [
            "blue", "purple", "red", "green", "teal", "pink", "orange", "yellow", "gray",
            "#000000", "#FFFFFF",
        ] {
            let color = Rgb::parse(name).unwrap();
            let dark = color.dark();
            assert!((dark.luminance() + 0.05) / (Rgb([24, 29, 39]).luminance() + 0.05) >= 4.5);
        }
    }
}
