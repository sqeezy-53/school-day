use gtk::glib;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::fmt::Write;

fn newlines(markup: &mut String, count: usize) {
    if !markup.is_empty() {
        let existing = markup
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\n')
            .count();
        markup.extend(std::iter::repeat_n('\n', count.saturating_sub(existing)));
    }
}

fn web_uri(url: &str) -> bool {
    !url.chars().any(char::is_control)
        && glib::Uri::parse(url, glib::UriFlags::NONE).is_ok_and(|uri| {
            matches!(uri.scheme().as_str(), "https" | "http")
                && uri.host().is_some_and(|host| !host.is_empty())
                && uri.userinfo().is_none()
        })
}

pub(super) fn markup(body: &str) -> String {
    let mut markup = String::new();
    let mut lists = Vec::new();
    let mut link = false;
    for event in Parser::new(body) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                newlines(&mut markup, 2);
                markup.push_str(if level as u8 <= 2 { "<big><b>" } else { "<b>" });
            }
            Event::End(TagEnd::Heading(level)) => {
                markup.push_str(if level as u8 <= 2 {
                    "</b></big>"
                } else {
                    "</b>"
                });
                newlines(&mut markup, 2);
            }
            Event::Start(Tag::Strong) => markup.push_str("<b>"),
            Event::End(TagEnd::Strong) => markup.push_str("</b>"),
            Event::Start(Tag::Emphasis) => markup.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => markup.push_str("</i>"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = web_uri(&dest_url);
                if link {
                    let _ = write!(
                        markup,
                        "<a href=\"{}\">",
                        glib::markup_escape_text(&dest_url)
                    );
                }
            }
            Event::End(TagEnd::Link) if link => markup.push_str("</a>"),
            Event::Start(Tag::List(start)) => lists.push(start),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                newlines(&mut markup, if lists.is_empty() { 2 } else { 1 });
            }
            Event::Start(Tag::Item) => {
                newlines(&mut markup, 1);
                markup.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                if let Some(Some(number)) = lists.last_mut() {
                    let _ = write!(markup, "{number}. ");
                    *number = number.saturating_add(1);
                } else {
                    markup.push_str("• ");
                }
            }
            Event::End(TagEnd::Item) => newlines(&mut markup, 1),
            Event::End(TagEnd::Paragraph) => {
                newlines(&mut markup, if lists.is_empty() { 2 } else { 1 })
            }
            Event::Start(Tag::CodeBlock(_)) => markup.push_str("<tt>"),
            Event::End(TagEnd::CodeBlock) => {
                markup.push_str("</tt>");
                newlines(&mut markup, 2);
            }
            Event::Code(code) => {
                let _ = write!(markup, "<tt>{}</tt>", glib::markup_escape_text(&code));
            }
            Event::SoftBreak | Event::HardBreak => markup.push('\n'),
            Event::Text(text) => markup.push_str(&glib::markup_escape_text(&text)),
            Event::Html(html) | Event::InlineHtml(html) => {
                let text = html
                    .replace("<details>", "")
                    .replace("</details>", "")
                    .replace("<summary>", "")
                    .replace("</summary>", "");
                markup.push_str(&glib::markup_escape_text(&text));
            }
            _ => {}
        }
    }
    markup.truncate(markup.trim_end().len());
    markup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_release_notes_and_escapes_untrusted_content() {
        let rendered = markup(
            "## Заголовок\n\n**Жирный** и *курсив*, `<код>`\nстрока\n\n- список\n\n3. номер\n\n[ссылка](https://example.org/?a=1&b=2) [опасно](javascript:alert)\n\n<script>bad()</script>",
        );
        for expected in [
            "<big><b>Заголовок</b></big>",
            "<b>Жирный</b> и <i>курсив</i>",
            "<tt>&lt;код&gt;</tt>\nстрока",
            "• список",
            "3. номер",
            "<a href=\"https://example.org/?a=1&amp;b=2\">ссылка</a> опасно",
            "&lt;script&gt;bad()&lt;/script&gt;",
        ] {
            assert!(rendered.contains(expected), "{rendered}");
        }
        assert!(!rendered.contains("javascript:"));
    }
}
