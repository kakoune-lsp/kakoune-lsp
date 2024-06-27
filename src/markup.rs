#[cfg(test)]
use indoc::indoc;
use lsp_types::*;
use pulldown_cmark::{Event, Parser, Tag};
use std::fmt::Write as _;

pub const FACE_INFO_DEFAULT: &str = "InfoDefault";

pub const FACE_INFO_BLOCK_QUOTE: &str = "InfoBlockQuote";
pub const FACE_INFO_BLOCK: &str = "InfoBlock";
pub const FACE_INFO_HEADER: &str = "InfoHeader";
pub const FACE_INFO_LINK_MONO: &str = "InfoLinkMono";
pub const FACE_INFO_LINK: &str = "InfoLink";
pub const FACE_INFO_LIST_ITEM: &str = "InfoBullet";
pub const FACE_INFO_MONO: &str = "InfoMono";
pub const FACE_INFO_RULE: &str = "InfoRule";

pub const FACE_INFO_DIAGNOSTIC_ERROR: &str = "InfoDiagnosticError";
pub const FACE_INFO_DIAGNOSTIC_HINT: &str = "InfoDiagnosticHint";
pub const FACE_INFO_DIAGNOSTIC_INFO: &str = "InfoDiagnosticInformation";
pub const FACE_INFO_DIAGNOSTIC_WARNING: &str = "InfoDiagnosticWarning";

/// Espace backslashes and opening braces for Kakoune markup strings
pub fn escape_kakoune_markup(s: &str) -> String {
    s.replace('\\', r"\\").replace('{', r"\{")
}

/// Transpile Markdown into Kakoune's markup syntax using faces for highlighting
pub fn markdown_to_kakoune_markup<S: AsRef<str>>(markdown: S) -> String {
    let markdown = markdown.as_ref();
    let parser = Parser::new(markdown);
    let mut markup = String::with_capacity(markdown.len());

    // State to indicate a code block
    let mut is_codeblock = false;
    // State to indicate a block quote
    let mut is_blockquote = false;
    // State to indicate a table
    let mut is_table = false;
    // State to indicate that at least one text line in a block quote
    // has been emitted
    let mut has_blockquote_text = false;
    // A stack to track nested lists.
    // The value tracks ordered vs unordered and the current entry number.
    let mut list_stack: Vec<Option<u64>> = vec![];
    // A stack to track the current 'base' face.
    // Certain tags can be nested, in which case it is not correct to just emit `{default}`
    // when the inner tag ends. Markdown example: ``[`code` link](...)``
    // The stack allows to track whatever face a closing tag needs to emit.
    let mut face_stack: Vec<String> = vec![];

    /// Get the current base face, either the top face on the stack
    /// or a fallback
    fn base_face(stack: &[String]) -> &str {
        stack
            .last()
            .map(|s| s.as_str())
            .unwrap_or(FACE_INFO_DEFAULT)
    }

    /// Removes the top most face from the stack, then returns the next entry
    /// as the current base face or a fallback
    fn pop_base_face(stack: &mut Vec<String>) -> &str {
        stack.pop();
        base_face(stack)
    }

    for e in parser {
        match e {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    // Block quotes with empty lines are parsed into paragraphes.
                    // However, even for the first of such paragraphs, `Tag::Blockquote`
                    // is emitted first.
                    // Since we don't want two `>` at the start, we need to wait for the text first.
                    if is_blockquote && has_blockquote_text {
                        markup.push('>');
                    }
                    markup.push('\n')
                }
                Tag::Heading(level, _, _) => {
                    face_stack.push(FACE_INFO_HEADER.into());
                    // Color as `{header}` but keep the Markdown syntax to visualize the header level
                    let _ = write!(
                        markup,
                        "\n{{{}}}{} ",
                        FACE_INFO_HEADER,
                        "#".repeat(level as usize)
                    );
                }
                Tag::BlockQuote => {
                    face_stack.push(FACE_INFO_BLOCK_QUOTE.into());
                    let _ = write!(markup, "{{{}}}", FACE_INFO_BLOCK_QUOTE);
                    is_blockquote = true
                }
                Tag::CodeBlock(_) => {
                    is_codeblock = true;
                    face_stack.push(FACE_INFO_BLOCK.into());
                    let _ = write!(markup, "\n{{{}}}", FACE_INFO_BLOCK);
                }
                Tag::List(num) => list_stack.push(num),
                Tag::Item => {
                    let base_face = base_face(&face_stack);

                    let list_level = list_stack.len();
                    // The parser shouldn't allow this to be empty
                    let item = list_stack.pop().expect("Tag::Item before Tag::List");

                    if let Some(num) = item {
                        let _ = write!(
                            markup,
                            "\n{}{{{}}}{}. {{{}}}",
                            "  ".repeat(list_level),
                            FACE_INFO_LIST_ITEM,
                            num,
                            base_face
                        );
                        // We need to keep track of the entry number ourselves.
                        list_stack.push(Some(num + 1));
                    } else {
                        let _ = write!(
                            markup,
                            "\n{}{{{}}}- {{{}}}",
                            "  ".repeat(list_level),
                            FACE_INFO_LIST_ITEM,
                            base_face
                        );
                        list_stack.push(item);
                    }
                }
                Tag::Emphasis => {
                    let _ = write!(markup, "{{+i@{}}}", base_face(&face_stack));
                }
                Tag::Strong => {
                    let _ = write!(markup, "{{+b@{}}}", base_face(&face_stack));
                }
                Tag::Strikethrough => {
                    let _ = write!(markup, "{{+s@{}}}", base_face(&face_stack));
                }
                // Kakoune doesn't support clickable links and the URL might be too long to show
                // nicely.
                // We'll only show the link title for now, which should be enough to search in the
                // relevant resource.
                Tag::Link(_, _, _) => {
                    face_stack.push(FACE_INFO_LINK.into());
                    let _ = write!(markup, "{{{}}}", FACE_INFO_LINK);
                }
                Tag::Image(_, _, _) => (),
                tag => warn!("Unsupported Markdown tag: {:?}", tag),
            },
            Event::End(t) => match t {
                Tag::Paragraph => markup.push('\n'),
                Tag::Heading(_, _, _) => {
                    let base_face = pop_base_face(&mut face_stack);
                    let _ = writeln!(markup, "{{{}}}", base_face);
                }
                Tag::BlockQuote => {
                    has_blockquote_text = false;
                    is_blockquote = false;
                    let base_face = pop_base_face(&mut face_stack);
                    let _ = write!(markup, "{{{}}}", base_face);
                }
                Tag::CodeBlock(_) => {
                    is_codeblock = false;
                    let base_face = pop_base_face(&mut face_stack);
                    let _ = write!(markup, "{{{}}}", base_face);
                }
                Tag::List(_) => {
                    // `.pop()` shouldn't fail here, unless the parser is having issues
                    list_stack
                        .pop()
                        .expect("Event::End(Tag::List) before Event::Start(Tag::List)");
                    if list_stack.is_empty() {
                        markup.push('\n');
                    }
                }
                Tag::Item => (),
                Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link(_, _, _) => {
                    let base_face = pop_base_face(&mut face_stack);
                    let _ = write!(markup, "{{{}}}", base_face);
                }
                Tag::Image(_, _, _) => (),
                tag => warn!("Unsupported Markdown tag: {:?}", tag),
            },
            Event::Text(text) => {
                is_table = text.starts_with('|');
                if is_blockquote {
                    has_blockquote_text = true;
                    markup.push_str("> ")
                }
                markup.push_str(&escape_kakoune_markup(&text))
            }
            Event::Code(c) => {
                let base_face = base_face(&face_stack);
                let face = if base_face == FACE_INFO_LINK {
                    FACE_INFO_LINK_MONO
                } else {
                    FACE_INFO_MONO
                };

                let _ = write!(
                    markup,
                    "{{{}}}{}{{{}}}",
                    face,
                    escape_kakoune_markup(&c),
                    base_face
                );
            }
            Event::Html(html) => markup.push_str(&escape_kakoune_markup(&html)),
            Event::FootnoteReference(_) => warn!("Unsupported Markdown event: {:?}", e),
            // Soft breaks should be kept in `<pre>`-style blocks.
            // Anywhere else, let the renderer handle line breaks.
            Event::SoftBreak => {
                if is_blockquote || is_codeblock || is_table {
                    markup.push('\n')
                } else {
                    markup.push(' ')
                }
            }
            Event::HardBreak => markup.push('\n'),
            // We don't know the size of the final render area, so we'll stick to rendering
            // Markdown syntax.
            Event::Rule => {
                let base_face = base_face(&face_stack);
                let _ = write!(markup, "\n{{{}}}---{{{}}}\n", FACE_INFO_RULE, base_face);
            }
            Event::TaskListMarker(_) => warn!("Unsupported Markdown event: {:?}", e),
        }
    }

    // Trim trailing whitespace. In some cases a face has been added after the trailing whitespace,
    // so we need to strip that first.
    markup
        .strip_suffix(&format!("{{{}}}", FACE_INFO_DEFAULT))
        .unwrap_or(&markup)
        .trim()
        .to_string()
}

/// Transpile the contents of an `lsp_types::MarkedString` into Kakoune markup
pub fn marked_string_to_kakoune_markup(contents: MarkedString) -> String {
    match contents {
        MarkedString::String(s) => markdown_to_kakoune_markup(s),
        MarkedString::LanguageString(s) => {
            format!(
                "{{{}}}{}{{{}}}",
                FACE_INFO_BLOCK,
                escape_kakoune_markup(&s.value),
                FACE_INFO_DEFAULT
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_kakoune_markup() {
        let markup = markdown_to_kakoune_markup(indoc!(
            r#"# heading
               body"#
        ));
        assert_eq!(
            markup,
            indoc!(
                r#"{InfoHeader}# heading{InfoDefault}

                   body"#
            )
        );
    }
}
