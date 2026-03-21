//! HTML renderer — wraps markdown in a styled HTML page.
//!
//! Uses pulldown-cmark to convert markdown to HTML, then wraps it
//! in a minimal dark-themed page that looks good in any browser.

use pulldown_cmark::{html, Options, Parser};

/// Convert markdown to a complete HTML page with dark theme styling.
pub fn markdown_to_html_page(markdown: &str, title: &str) -> String {
    // Parse markdown with common extensions
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(markdown, options);
    let mut html_body = String::new();
    html::push_html(&mut html_body, parser);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{
    --bg: #1a1b26;
    --fg: #c0caf5;
    --accent: #7aa2f7;
    --border: #2f3348;
    --surface: #24283b;
    --muted: #565f89;
    --green: #9ece6a;
    --red: #f7768e;
  }}
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: var(--bg);
    color: var(--fg);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    font-size: 15px;
    line-height: 1.6;
    max-width: 900px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }}
  h1 {{ color: var(--accent); font-size: 1.5rem; margin-bottom: 1.5rem; border-bottom: 1px solid var(--border); padding-bottom: 0.5rem; }}
  h2 {{ color: var(--accent); font-size: 1.2rem; margin-top: 1.5rem; margin-bottom: 0.5rem; }}
  hr {{ border: none; border-top: 1px solid var(--border); margin: 1.5rem 0; }}
  p {{ margin-bottom: 0.8rem; }}
  strong {{ color: var(--green); }}
  em {{ color: var(--muted); font-style: italic; }}
  blockquote {{
    border-left: 3px solid var(--accent);
    padding: 0.5rem 1rem;
    margin: 0.5rem 0;
    background: var(--surface);
    border-radius: 0 4px 4px 0;
  }}
  blockquote p {{ margin-bottom: 0.3rem; }}
  pre {{
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.8rem 1rem;
    overflow-x: auto;
    margin: 0.5rem 0 1rem 0;
    font-size: 13px;
  }}
  code {{
    font-family: 'Cascadia Code', 'Fira Code', 'JetBrains Mono', ui-monospace, monospace;
    font-size: 13px;
  }}
  p code {{
    background: var(--surface);
    padding: 0.15rem 0.35rem;
    border-radius: 3px;
    border: 1px solid var(--border);
  }}
  details {{
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin: 0.5rem 0 1rem 0;
    padding: 0.5rem 1rem;
  }}
  details summary {{
    cursor: pointer;
    color: var(--muted);
    font-size: 13px;
  }}
  details summary:hover {{ color: var(--accent); }}
  a {{ color: var(--accent); text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
  table {{
    border-collapse: collapse;
    margin: 1rem 0;
    width: 100%;
  }}
  th, td {{
    border: 1px solid var(--border);
    padding: 0.4rem 0.8rem;
    text-align: left;
  }}
  th {{ background: var(--surface); color: var(--accent); }}
</style>
</head>
<body>
{html_body}
</body>
</html>"#,
        title = title,
        html_body = html_body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_markdown_heading() {
        let html = markdown_to_html_page("# Hello", "Test");
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<title>Test</title>"));
    }

    #[test]
    fn renders_blockquote() {
        let html = markdown_to_html_page("> quoted text", "Test");
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("quoted text"));
    }

    #[test]
    fn renders_code_block() {
        let html = markdown_to_html_page("```rust\nfn main() {}\n```", "Test");
        assert!(html.contains("<pre>"));
        assert!(html.contains("fn main()"));
    }

    #[test]
    fn includes_dark_theme_styles() {
        let html = markdown_to_html_page("hello", "Test");
        assert!(html.contains("--bg: #1a1b26"));
        assert!(html.contains("--accent: #7aa2f7"));
    }

    #[test]
    fn html_page_is_complete() {
        let html = markdown_to_html_page("test", "My Title");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
    }
}
