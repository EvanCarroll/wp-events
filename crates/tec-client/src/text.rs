//! HTML-to-text conversion for rendered WordPress fields.

/// Strip tags, turn block-level closers and `<br>` into newlines, decode entities, and
/// collapse runs of spaces. Script and style contents are dropped.
pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    // Raw newlines/tabs in HTML source are plain whitespace; only tags create line breaks.
    let ws = |s: &str| s.replace(['\n', '\r', '\t'], " ");
    while let Some(lt) = rest.find('<') {
        out.push_str(&ws(&rest[..lt]));
        let after = &rest[lt + 1..];
        let Some(gt) = after.find('>') else {
            out.push_str(&ws(&rest[lt..]));
            rest = "";
            break;
        };
        let tag = after[..gt].trim().to_ascii_lowercase();
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        rest = &after[gt + 1..];
        if !tag.starts_with('/') && (name == "script" || name == "style") {
            let close = format!("</{name}");
            match rest.to_ascii_lowercase().find(&close) {
                Some(i) => rest = rest[i..].find('>').map_or("", |j| &rest[i + j + 1..]),
                None => rest = "",
            }
            continue;
        }
        let block = matches!(
            name.as_str(),
            "br" | "p"
                | "div"
                | "li"
                | "ul"
                | "ol"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "tr"
                | "blockquote"
        );
        if block && (name == "br" || tag.starts_with('/') || name == "li") {
            out.push('\n');
        }
    }
    out.push_str(&ws(rest));
    let decoded = html_escape::decode_html_entities(&out);
    // Collapse spaces within lines, drop empty lines and zero-width spaces.
    decoded
        .replace('\u{200b}', "")
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every `<a href>` in document order, as `(label, href)`: the label is the anchor's text (see
/// [`html_to_text`]) and the href has its entities decoded. Anchors without an `href` are skipped.
pub(crate) fn html_links(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("<a") {
        let open = at + i;
        let after_name = open + 2;
        // `<abbr>`, `<area>`, `<aside>`: not an anchor.
        if !lower[after_name..].starts_with(|c: char| c.is_ascii_whitespace() || c == '>') {
            at = after_name;
            continue;
        }
        let Some(gt) = lower[open..].find('>') else {
            break;
        };
        let tag = &html[open..open + gt];
        let body_start = open + gt + 1;
        let body_end = lower[body_start..]
            .find("</a")
            .map_or(html.len(), |j| body_start + j);
        if let Some(href) = attr(tag, "href") {
            let href = html_escape::decode_html_entities(&href).trim().to_owned();
            if !href.is_empty() {
                out.push((html_to_text(&html[body_start..body_end]), href));
            }
        }
        at = body_end;
    }
    out
}

/// The value of attribute `name` in a start tag, quoted or bare.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find(name) {
        let start = at + i;
        let before_ok = lower[..start].ends_with(|c: char| c.is_ascii_whitespace());
        let rest = lower[start + name.len()..].trim_start();
        if before_ok && let Some(value) = rest.strip_prefix('=') {
            let offset = tag.len() - value.len();
            let value = tag[offset..].trim_start();
            return Some(match value.chars().next()? {
                q @ ('"' | '\'') => value[1..].split(q).next()?.to_owned(),
                _ => value
                    .split(|c: char| c.is_ascii_whitespace() || c == '>')
                    .next()?
                    .to_owned(),
            });
        }
        at = start + name.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{html_links, html_to_text};

    #[test]
    fn links_keep_label_and_href() {
        let html = "<p>Register <a class=\"x\" href=\"https://ex.com/r?a=1&amp;b=2\">here &amp; now</a>, \
                    or <A HREF='https://ex.com/b'>there</A>.</p><abbr title=\"no\">n</abbr>\
                    <a name=\"anchor\">no href</a><a href=https://ex.com/bare>bare</a>";
        assert_eq!(
            html_links(html),
            vec![
                ("here & now".to_owned(), "https://ex.com/r?a=1&b=2".to_owned()),
                ("there".to_owned(), "https://ex.com/b".to_owned()),
                ("bare".to_owned(), "https://ex.com/bare".to_owned()),
            ]
        );
    }

    #[test]
    fn strips_and_decodes() {
        let html = "<p><em><strong>Register here: <a href=\"x\">link</a></strong></em></p>\n<p>Black Futures Market &#8211; a showcase<br />\nThe 713 Connection</p><ul><li>One</li><li>Two &amp; three</li></ul><script>alert(1)</script>";
        assert_eq!(
            html_to_text(html),
            "Register here: link\nBlack Futures Market \u{2013} a showcase\nThe 713 Connection\nOne\nTwo & three"
        );
    }
}
