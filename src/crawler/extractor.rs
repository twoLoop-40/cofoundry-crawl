use crate::config::PageMetadata;
use scraper::{Html, Selector};
use url::Url;

/// Extraction result from HTML
pub struct Extracted {
    pub title: Option<String>,
    pub markdown: String,
    pub links: Vec<String>,
    pub metadata: PageMetadata,
}

pub fn empty_metadata() -> PageMetadata {
    PageMetadata {
        content_type: None,
        content_length: None,
        description: None,
        keywords: vec![],
        h1: vec![],
        h2: vec![],
    }
}

/// Extract content from HTML → Markdown + links + metadata
/// (CrawlerLLMExtractionSpec §1: content extraction)
pub fn extract(html: &str, base_url: &str) -> Extracted {
    let doc = Html::parse_document(html);
    let base = Url::parse(base_url).ok();

    let title = extract_title(&doc);
    let metadata = extract_metadata(&doc);
    let links = extract_links(&doc, base.as_ref());
    let markdown = html_to_markdown(&doc);

    Extracted {
        title,
        markdown,
        links,
        metadata,
    }
}

fn extract_title(doc: &Html) -> Option<String> {
    let sel = Selector::parse("title").ok()?;
    doc.select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
}

fn extract_metadata(doc: &Html) -> PageMetadata {
    let mut meta = empty_metadata();

    // Description
    if let Ok(sel) = Selector::parse("meta[name='description']") {
        if let Some(el) = doc.select(&sel).next() {
            meta.description = el.value().attr("content").map(|s| s.to_string());
        }
    }

    // Keywords
    if let Ok(sel) = Selector::parse("meta[name='keywords']") {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(content) = el.value().attr("content") {
                meta.keywords = content.split(',').map(|s| s.trim().to_string()).collect();
            }
        }
    }

    // H1, H2
    if let Ok(sel) = Selector::parse("h1") {
        meta.h1 = doc
            .select(&sel)
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Ok(sel) = Selector::parse("h2") {
        meta.h2 = doc
            .select(&sel)
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    meta
}

fn extract_links(doc: &Html, base: Option<&Url>) -> Vec<String> {
    let sel = match Selector::parse("a[href]") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    doc.select(&sel)
        .filter_map(|el| {
            let href = el.value().attr("href")?;
            resolve_url(href, base)
        })
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .collect()
}

fn resolve_url(href: &str, base: Option<&Url>) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    // Skip javascript:, mailto:, tel:, #
    if href.starts_with("javascript:")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with('#')
    {
        return None;
    }
    base.and_then(|b| b.join(href).ok().map(|u| u.to_string()))
}

/// Simple HTML → Markdown conversion (headings, paragraphs, lists, links)
fn html_to_markdown(doc: &Html) -> String {
    let mut output = String::new();

    // Extract text from body
    let body_sel = Selector::parse("body").ok();
    let root = if let Some(ref sel) = body_sel {
        doc.select(sel).next()
    } else {
        None
    };

    if let Some(body) = root {
        collect_text(&mut output, &body);
    } else {
        // Fallback: just get all text
        output = doc.root_element().text().collect::<Vec<_>>().join(" ");
    }

    // Clean up excessive whitespace
    let lines: Vec<&str> = output.lines().map(|l| l.trim()).collect();
    let mut result = String::new();
    let mut prev_empty = false;
    for line in lines {
        if line.is_empty() {
            if !prev_empty {
                result.push('\n');
                prev_empty = true;
            }
        } else {
            result.push_str(line);
            result.push('\n');
            prev_empty = false;
        }
    }

    result.trim().to_string()
}

fn collect_text(output: &mut String, element: &scraper::ElementRef) {
    use scraper::Node;

    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                let t = text.trim();
                if !t.is_empty() {
                    output.push_str(t);
                    output.push(' ');
                }
            }
            Node::Element(el) => {
                let tag = el.name();
                let child_ref = scraper::ElementRef::wrap(child);
                match tag {
                    "h1" => {
                        output.push_str("\n# ");
                        if let Some(ref cr) = child_ref {
                            collect_text(output, cr);
                        }
                        output.push('\n');
                    }
                    "h2" => {
                        output.push_str("\n## ");
                        if let Some(ref cr) = child_ref {
                            collect_text(output, cr);
                        }
                        output.push('\n');
                    }
                    "h3" => {
                        output.push_str("\n### ");
                        if let Some(ref cr) = child_ref {
                            collect_text(output, cr);
                        }
                        output.push('\n');
                    }
                    "p" | "div" | "section" | "article" => {
                        output.push('\n');
                        if let Some(ref cr) = child_ref {
                            collect_text(output, cr);
                        }
                        output.push('\n');
                    }
                    "li" => {
                        output.push_str("- ");
                        if let Some(ref cr) = child_ref {
                            collect_text(output, cr);
                        }
                        output.push('\n');
                    }
                    "br" => output.push('\n'),
                    "script" | "style" | "noscript" => {} // skip
                    _ => {
                        if let Some(ref cr) = child_ref {
                            collect_text(output, cr);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        let html = "<html><head><title>Test Page</title></head><body></body></html>";
        let result = extract(html, "https://example.com");
        assert_eq!(result.title, Some("Test Page".to_string()));
    }

    #[test]
    fn test_extract_links() {
        let html = r#"<html><body>
            <a href="https://example.com/page1">Page 1</a>
            <a href="/page2">Page 2</a>
            <a href="javascript:void(0)">JS</a>
        </body></html>"#;
        let result = extract(html, "https://example.com");
        assert_eq!(result.links.len(), 2);
        assert!(result.links.contains(&"https://example.com/page1".to_string()));
        assert!(result.links.contains(&"https://example.com/page2".to_string()));
    }

    #[test]
    fn test_extract_metadata() {
        let html = r#"<html><head>
            <meta name="description" content="A test page">
            <meta name="keywords" content="test, page, example">
        </head><body>
            <h1>Main Title</h1>
            <h2>Sub Title</h2>
        </body></html>"#;
        let result = extract(html, "https://example.com");
        assert_eq!(result.metadata.description, Some("A test page".to_string()));
        assert_eq!(result.metadata.keywords, vec!["test", "page", "example"]);
        assert_eq!(result.metadata.h1, vec!["Main Title"]);
        assert_eq!(result.metadata.h2, vec!["Sub Title"]);
    }

    #[test]
    fn test_html_to_markdown() {
        let html = r#"<html><body>
            <h1>Hello</h1>
            <p>This is a paragraph.</p>
            <ul><li>Item 1</li><li>Item 2</li></ul>
        </body></html>"#;
        let result = extract(html, "https://example.com");
        assert!(result.markdown.contains("# Hello"));
        assert!(result.markdown.contains("This is a paragraph."));
        assert!(result.markdown.contains("- Item 1"));
    }
}
