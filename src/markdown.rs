use pulldown_cmark::{Event, Parser, Tag};
use std::fmt::Write;

pub fn markdown_to_kak(text: &str) -> String {
    let mut out = String::new();
    let mut in_italic = false;
    let mut in_bold = false;
    let mut in_code_block = false;
    let mut in_blockquote = false;
    let emit_style = |out: &mut String, in_italic, in_bold| {
        write!(
            out,
            "{{{}{}{}}}",
            if in_italic || in_bold { "+" } else { "" },
            if in_italic { "i" } else { "" },
            if in_bold { "b" } else { "" },
        )
        .unwrap();
    };
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Paragraph) => {}
            Event::End(Tag::Paragraph) => {
                out.push_str("\n\n");
            }
            Event::Start(Tag::Heading(n)) => {
                (0..n).for_each(|_| out.push('#'));
                out.push(' ');
            }
            Event::End(Tag::Heading(_)) => {
                out.push('\n');
            }
            Event::Start(Tag::BlockQuote) => {
                in_blockquote = true;
            }
            Event::End(Tag::BlockQuote) => {
                in_blockquote = false;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
            }
            Event::End(Tag::CodeBlock(_)) => {
                in_code_block = false;
                out.push('\n');
            }
            Event::Start(Tag::List(_)) => {
                // TODO
            }
            Event::End(Tag::List(_)) => {
                // TODO
            }
            Event::Start(Tag::Item) => {
                out.push_str("- ");
            }
            Event::End(Tag::Item) => {}
            Event::Start(Tag::FootnoteDefinition(_)) => {}
            Event::End(Tag::FootnoteDefinition(_)) => {}
            Event::Start(Tag::Table(_))
            | Event::End(Tag::Table(_))
            | Event::Start(Tag::TableHead)
            | Event::End(Tag::TableHead)
            | Event::Start(Tag::TableRow)
            | Event::End(Tag::TableRow)
            | Event::Start(Tag::TableCell)
            | Event::End(Tag::TableCell) => {
                // Disabled extension
            }
            Event::Start(Tag::Emphasis) => {
                in_italic = true;
                emit_style(&mut out, in_italic, in_bold);
            }
            Event::End(Tag::Emphasis) => {
                in_italic = false;
                emit_style(&mut out, in_italic, in_bold);
            }
            Event::Start(Tag::Strong) => {
                in_bold = true;
                emit_style(&mut out, in_italic, in_bold);
            }
            Event::End(Tag::Strong) => {
                in_bold = false;
                emit_style(&mut out, in_italic, in_bold);
            }
            Event::Start(Tag::Strikethrough) | Event::End(Tag::Strikethrough) => {
                // Disabled extension
            }
            Event::Start(Tag::Link(..)) => {
                // TODO
            }
            Event::End(Tag::Link(..)) => {
                // TODO
            }
            Event::Start(Tag::Image(..)) => {
                // TODO
            }
            Event::End(Tag::Image(..)) => {
                // TODO
            }
            Event::Text(text) | Event::Html(text) | Event::FootnoteReference(text) => {
                if in_code_block {
                    for line in text.as_ref().split('\n').filter(|x| !x.is_empty()) {
                        out.push_str("  ");
                        out.push_str(line);
                        out.push('\n');
                    }
                } else if in_blockquote {
                    for line in text.as_ref().split('\n').filter(|x| !x.is_empty()) {
                        out.push_str("> ");
                        out.push_str(line);
                        out.push('\n');
                    }
                } else {
                    out.push_str(text.as_ref());
                }
            }
            Event::Code(text) => {
                for line in text.as_ref().split('\n').filter(|x| !x.is_empty()) {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Event::SoftBreak => {
                if !in_code_block && !in_blockquote {
                    out.push(' ');
                }
            }
            Event::HardBreak => {
                out.push('\n');
            }
            Event::Rule => out.push_str("\n---\n"),
            Event::TaskListMarker(_) => {
                // Disabled extension
            }
        }
    }
    out.trim_end_matches('\n').to_owned()
}

#[cfg(test)]
pub mod test {
    use super::markdown_to_kak;

    #[test]
    fn markdown() {
        let cases = [
            ("# Single header", "# Single header"),
            ("## Double header", "## Double header"),
            ("#Invalid header", "#Invalid header"),
            ("Paragraph A\n\n\nParagraph B", "Paragraph A\n\nParagraph B"),
            (
                "Paragraph A\n\n\n\nParagraph B",
                "Paragraph A\n\nParagraph B",
            ),
            ("Line A\nLine B", "Line A Line B"),
            ("```code block```", "  code block"),
            ("```\ncode block\n```", "  code block"),
            (
                "```\ncode block\nsecond line\nthird line\n```",
                "  code block\n  second line\n  third line",
            ),
            ("*italic*", "{+i}italic{}"),
            ("**bold**", "{+b}bold{}"),
            ("***bold italic***", "{+i}{+ib}bold italic{+i}{}"),
            (
                "***bold italic**just italic*",
                "{+i}{+ib}bold italic{+i}just italic{}",
            ),
            ("> blockquote single line", "> blockquote single line"),
            (
                ">blockquote single line no space",
                "> blockquote single line no space",
            ),
            ("> blockquote\n> multi line", "> blockquote\n> multi line"),
            ("https://some-uri.tld", "https://some-uri.tld"),
            ("<https://some-uri.tld>", "https://some-uri.tld"),
            ("[link name](https://some-uri.tld)", "link name"), // TODO: improve
        ];
        for (case, wanted) in cases.iter() {
            assert_eq!(*wanted, markdown_to_kak(case));
        }
    }
}
