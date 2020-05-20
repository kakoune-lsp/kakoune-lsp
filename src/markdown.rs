use pulldown_cmark::{Event, Parser, Tag};
use std::fmt::Write;

pub fn markdown_to_kak(text: &str) -> String {
    let mut out = String::new();
    let mut in_italic = false;
    let mut in_bold = false;
    let mut in_code_block = false;
    let mut in_blockquote = false;
    fn emit_style(out: &mut String, in_italic: bool, in_bold: bool) {
        write!(
            out,
            "{{{}{}{}}}",
            if in_italic || in_bold { "+" } else { "" },
            if in_italic { "i" } else { "" },
            if in_bold { "b" } else { "" },
        )
        .unwrap();
    }
    eprintln!("{:?}", text);
    for event in Parser::new(text) {
        eprintln!("  {:?}", event);
        match event {
            Event::Start(Tag::Paragraph) => {
                out.push('\n');
            }
            Event::End(Tag::Paragraph) => {
                out.push('\n');
            }
            Event::Start(Tag::Heading(n)) => {
                out.push('\n');
                out.extend((0..n).map(|_| '#'));
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
                        out.push_str("| ");
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
                out.push('`');
                out.push_str(text.as_ref());
                out.push('`');
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
    out.trim_start_matches('\n')
        .trim_end_matches('\n')
        .to_owned()
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
            ("`inline code`", "`inline code`"),
            ("```\ncode block\n```", "| code block"),
            (
                "```\ncode block\nsecond line\nthird line\n```",
                "| code block\n| second line\n| third line",
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
            (
                r#"```rust
alloc::string::String
pub fn push_str(&mut self, string: &str)
```

Appends a given string slice onto the end of this `String`.

# Examples

Basic usage:

```rust
let mut s = String::from("foo");

s.push_str("bar");

assert_eq!("foobar", s);
"#,
                r#"| alloc::string::String
| pub fn push_str(&mut self, string: &str)

Appends a given string slice onto the end of this `String`.

# Examples

Basic usage:
| let mut s = String::from("foo");
| s.push_str("bar");
| assert_eq!("foobar", s);"#,
            ),
        ];
        for (case, expected) in cases.iter() {
            let got = markdown_to_kak(case);
            println!("---\n{}\n---", got);
            assert_eq!(*expected, got);
        }
    }
}
