use crate::context::*;
use crate::position::*;
use crate::text_edit::*;
use crate::types::*;
use itertools::Itertools;
use lsp_types::*;
use pulldown_cmark::{Event, Parser, Tag};
use ropey::Rope;
use std::fs::File;
use std::io::{stderr, stdout, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use std::{collections::HashMap, path::Path};
use std::{env, fs, path, process, thread};

pub fn temp_dir() -> path::PathBuf {
    let mut path = env::temp_dir();
    path.push("kak-lsp");
    let old_mask = unsafe { libc::umask(0) };
    // Ignoring possible error during $TMPDIR/kak-lsp creation to have a chance to restore umask.
    let _ = fs::DirBuilder::new()
        .recursive(true)
        .mode(0o1777)
        .create(&path);
    unsafe {
        libc::umask(old_mask);
    }
    path.push(whoami::username());
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&path)
        .unwrap();
    path
}

pub struct TempFifo {
    pub path: String,
}

pub fn temp_fifo() -> Option<TempFifo> {
    let mut path = temp_dir();
    path.push(format!("{:x}", rand::random::<u64>()));
    let path = path.to_str().unwrap().to_string();
    let fifo_result = unsafe {
        let path = std::ffi::CString::new(path.clone()).unwrap();
        libc::mkfifo(path.as_ptr(), 0o600)
    };
    if fifo_result != 0 {
        return None;
    }
    Some(TempFifo { path })
}

impl Drop for TempFifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Represent list of symbol information as filetype=grep buffer content.
/// Paths are converted into relative to project root.
pub fn format_symbol_information(items: Vec<SymbolInformation>, ctx: &Context) -> String {
    items
        .into_iter()
        .map(|symbol| {
            let SymbolInformation {
                location,
                name,
                kind,
                ..
            } = symbol;
            let filename = location.uri.to_file_path().unwrap();
            let filename_str = filename.to_str().unwrap();
            let position = get_kakoune_position(filename_str, &location.range.start, ctx)
                .unwrap_or_else(|| KakounePosition {
                    line: location.range.start.line + 1,
                    column: location.range.start.character + 1,
                });
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                short_file_path(filename_str, &ctx.root_path),
                position.line,
                position.column,
                description
            )
        })
        .join("\n")
}

/// Represent list of document symbol as filetype=grep buffer content.
/// Paths are converted into relative to project root.

pub fn format_document_symbol(
    items: Vec<DocumentSymbol>,
    meta: &EditorMeta,
    ctx: &Context,
) -> String {
    items
        .into_iter()
        .map(|symbol| {
            let DocumentSymbol {
                range, name, kind, ..
            } = symbol;
            let position =
                get_kakoune_position(&meta.buffile, &range.start, ctx).unwrap_or_else(|| {
                    KakounePosition {
                        line: range.start.line + 1,
                        column: range.start.character + 1,
                    }
                });
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                short_file_path(&meta.buffile, &ctx.root_path),
                position.line,
                position.column,
                description
            )
        })
        .join("\n")
}

/// Escape Kakoune string wrapped into single quote
pub fn editor_escape(s: &str) -> String {
    s.replace("'", "''")
}

/// Convert to Kakoune string by wrapping into quotes and escaping
pub fn editor_quote(s: &str) -> String {
    format!("'{}'", editor_escape(s))
}

/// Espace opening braces for Kakoune markup strings
pub fn escape_brace(s: &str) -> String {
    s.replace("{", "\\{")
}

// Cleanup and gracefully exit
pub fn goodbye(session: &str, code: i32) {
    if code == 0 {
        let path = temp_dir();
        let sock_path = path.join(session);
        let pid_path = path.join(format!("{}.pid", session));
        if fs::remove_file(sock_path).is_err() {
            warn!("Failed to remove socket file");
        };
        if pid_path.exists() && fs::remove_file(pid_path).is_err() {
            warn!("Failed to remove pid file");
        };
    }
    stderr().flush().unwrap();
    stdout().flush().unwrap();
    // give stdio a chance to actually flush
    thread::sleep(Duration::from_secs(1));
    process::exit(code);
}

/// Convert language filetypes configuration into a more lookup-friendly form.
pub fn filetype_to_language_id_map(config: &Config) -> HashMap<String, String> {
    let mut filetypes = HashMap::default();
    for (language_id, language) in &config.language {
        for filetype in &language.filetypes {
            filetypes.insert(filetype.clone(), language_id.clone());
        }
    }
    filetypes
}

/// Wrapper for kakoune_position_to_lsp which uses context to get buffer content and offset encoding.
pub fn get_lsp_position(
    filename: &str,
    position: &KakounePosition,
    ctx: &Context,
) -> Option<Position> {
    ctx.documents
        .get(filename)
        .map(|document| kakoune_position_to_lsp(position, &document.text, ctx.offset_encoding))
}

/// Wrapper for lsp_position_to_kakoune which uses context to get buffer content and offset encoding.
/// Reads the file directly if it is not present in context (is not open in editor).
pub fn get_kakoune_position(
    filename: &str,
    position: &Position,
    ctx: &Context,
) -> Option<KakounePosition> {
    get_file_contents(filename, ctx)
        .map(|text| lsp_position_to_kakoune(position, &text, ctx.offset_encoding))
}

/// Apply text edits to the file pointed by uri either by asking Kakoune to modify corresponding
/// buffer or by editing file directly when it's not open in editor.
pub fn apply_text_edits(meta: &EditorMeta, uri: &Url, edits: Vec<TextEdit>, ctx: &Context) {
    let edits = edits.into_iter().map(OneOf::Left).collect::<Vec<_>>();
    apply_annotated_text_edits(meta, uri, &edits, ctx)
}

/// Apply text edits to the file pointed by uri either by asking Kakoune to modify corresponding
/// buffer or by editing file directly when it's not open in editor.
pub fn apply_annotated_text_edits(
    meta: &EditorMeta,
    uri: &Url,
    edits: &[OneOf<TextEdit, AnnotatedTextEdit>],
    ctx: &Context,
) {
    if let Some(document) = uri
        .to_file_path()
        .ok()
        .and_then(|path| path.to_str().and_then(|buffile| ctx.documents.get(buffile)))
    {
        let meta = meta.clone();
        match apply_text_edits_to_buffer(
            &meta.client,
            Some(uri),
            edits,
            &document.text,
            ctx.offset_encoding,
        ) {
            Some(cmd) => ctx.exec(meta, cmd),
            // Nothing to do, but sending command back to the editor is required to handle case when
            // editor is blocked waiting for response via fifo.
            None => ctx.exec(meta, "nop"),
        }
    } else if let Err(e) = apply_text_edits_to_file(uri, edits, ctx.offset_encoding) {
        error!("Failed to apply edits to file {} ({})", uri, e);
    }
}

/// Get the contents of a file.
/// Searches ctx.documents first and falls back to reading the file directly.
pub fn get_file_contents(filename: &str, ctx: &Context) -> Option<Rope> {
    ctx.documents
        .get(filename)
        .map(|doc| doc.text.clone())
        .or_else(|| {
            File::open(filename)
                .ok()
                .and_then(|f| Rope::from_reader(BufReader::new(f)).ok())
        })
}

pub fn short_file_path<'a>(target: &'a str, current_dir: &str) -> &'a str {
    Path::new(target)
        .strip_prefix(current_dir)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or(target)
}

/// Get the current base face, either the top face on the stack
/// or a fallback
fn get_base_face<'a>(stack: &'a Vec<String>, default: &'a str) -> &'a str {
    stack.last().map(|s| s.as_str()).unwrap_or(default)
}

/// Removes the top most face from the stack, then returns the next entry
/// as the current base face or a fallback
fn pop_base_face<'a>(stack: &'a mut Vec<String>, default: &'a str) -> &'a str {
    stack.pop();
    get_base_face(stack, default)
}

const FACE_DEFAULT: &str = "InfoDefault";
const FACE_HEADER: &str = "InfoHeader";
const FACE_BLOCK: &str = "InfoBlock";
const FACE_LIST_ITEM: &str = "InfoBullet";
const FACE_LINK: &str = "InfoLink";
const FACE_MONO: &str = "InfoMono";
const FACE_LINK_MONO: &str = "InfoLinkMono";
const FACE_RULE: &str = "InfoRule";

/// Parses Markdown into Kakoune's markup syntax using faces for highlighting
pub fn markdown_to_kakoune_markup<S: AsRef<str>>(markdown: S) -> String {
    let markdown = markdown.as_ref();
    let parser = Parser::new(markdown);
    let mut markup = String::with_capacity(markdown.len());

    // State to indicate a code block
    let mut is_codeblock = false;
    // State to indicate a block quote
    let mut is_blockquote = false;
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
                Tag::Heading(level) => {
                    face_stack.push(FACE_HEADER.into());
                    // Color as `{header}` but keep the Markdown syntax to visualize the header level
                    markup.push_str(&format!(
                        "\n{{{}}}{} ",
                        FACE_HEADER,
                        "#".repeat(level as usize)
                    ))
                }
                Tag::CodeBlock(_) => {
                    is_codeblock = true;
                    face_stack.push(FACE_BLOCK.into());
                    markup.push_str(&format!("\n{{{}}}", FACE_BLOCK))
                }
                Tag::List(num) => list_stack.push(num),
                Tag::Item => {
                    let base_face = face_stack
                        .last()
                        .map(|s| s.as_str())
                        .unwrap_or(FACE_DEFAULT);

                    let list_level = list_stack.len();
                    // The parser shouldn't allow this to be empty
                    let item = list_stack.pop().expect("Tag::Item before Tag::List");

                    if let Some(num) = item {
                        markup.push_str(&format!(
                            "\n{}{{{}}}{}. {{{}}}",
                            "  ".repeat(list_level),
                            FACE_LIST_ITEM,
                            num,
                            base_face
                        ));
                        // We need to keep track of the entry number ourselves.
                        list_stack.push(Some(num + 1));
                    } else {
                        markup.push_str(&format!(
                            "\n{}{{{}}}- {{{}}}",
                            "  ".repeat(list_level),
                            FACE_LIST_ITEM,
                            base_face
                        ));
                        list_stack.push(item);
                    }
                }
                Tag::Emphasis => {
                    let base_face = get_base_face(&face_stack, FACE_DEFAULT);
                    markup.push_str(&format!("{{+i@{}}}", base_face))
                }
                Tag::Strong => {
                    let base_face = get_base_face(&face_stack, FACE_DEFAULT);
                    markup.push_str(&format!("{{+b@{}}}", base_face))
                }
                // Kakoune doesn't support clickable links and the URL might be too long to show
                // nicely.
                // We'll only show the link title for now, which should be enough to search in the
                // relevant resource.
                Tag::Link(_, _, _) => {
                    face_stack.push(FACE_LINK.into());
                    markup.push_str(&format!("{{{}}}", FACE_LINK))
                }
                Tag::BlockQuote => is_blockquote = true,
                _ => {}
            },
            Event::End(t) => match t {
                Tag::Paragraph => markup.push('\n'),
                Tag::List(_) => {
                    // The parser shouldn't allow this to be empty
                    list_stack
                        .pop()
                        .expect("Event::End(Tag::List) before Event::Start(Tag::List)");
                    if list_stack.is_empty() {
                        markup.push('\n');
                    }
                }
                Tag::CodeBlock(_) => {
                    is_codeblock = false;
                    let base_face = pop_base_face(&mut face_stack, FACE_DEFAULT);
                    markup.push_str(&format!("{{{}}}", base_face));
                }
                Tag::BlockQuote => {
                    has_blockquote_text = false;
                    is_blockquote = false
                }
                Tag::Heading(_) => {
                    let base_face = pop_base_face(&mut face_stack, FACE_DEFAULT);
                    markup.push_str(&format!("{{{}}}\n", base_face));
                }
                Tag::Emphasis | Tag::Strong | Tag::Link(_, _, _) => {
                    let base_face = pop_base_face(&mut face_stack, FACE_DEFAULT);
                    markup.push_str(&format!("{{{}}}", base_face));
                }
                _ => {}
            },
            Event::Code(c) => {
                let base_face = get_base_face(&face_stack, FACE_DEFAULT);
                let face = if base_face == FACE_LINK {
                    FACE_LINK_MONO
                } else {
                    FACE_MONO
                };

                markup.push_str(&format!(
                    "{{{}}}{}{{{}}}",
                    face,
                    escape_brace(&c),
                    base_face
                ))
            }
            Event::Text(text) => {
                if is_blockquote {
                    has_blockquote_text = true;
                    markup.push_str("> ")
                }
                markup.push_str(&escape_brace(&text))
            }
            Event::Html(html) => markup.push_str(&escape_brace(&html)),
            // We don't know the size of the final render area, so we'll stick to just Markdown
            // syntax.
            Event::Rule => {
                let base_face = get_base_face(&face_stack, FACE_DEFAULT);
                markup.push_str(&format!("\n{{{}}}---{{{}}}\n", FACE_RULE, base_face));
            }
            // Soft breaks should be kept in `<pre>`-style blocks.
            // Anywhere else, let the renderer handle line breaks.
            Event::SoftBreak => {
                if is_blockquote || is_codeblock {
                    markup.push('\n')
                } else {
                    markup.push(' ')
                }
            }
            Event::HardBreak => markup.push('\n'),
            _ => {}
        }
    }

    // Trim trailing whitespace and add a `{default}` at the end,
    // to prevent bleeding markup when concatenated with other text.
    // In some cases a `{default}` has been added after the trailing whitespace,
    // so we need to strip that first.
    let mut markup = markup
        .strip_suffix(&format!("{{{}}}", FACE_DEFAULT))
        .unwrap_or(&markup)
        .trim()
        .to_owned();
    markup.push_str(&format!("{{{}}}", FACE_DEFAULT));
    markup
}

/// Parse the contents of a `lsp_types::MarkedString` into Kakoune markup
pub fn parse_marked_string(contents: MarkedString) -> String {
    match contents {
        MarkedString::String(s) => markdown_to_kakoune_markup(s),
        MarkedString::LanguageString(s) => {
            format!("{{{}}}{}{{{}}}", FACE_BLOCK, s.value, FACE_DEFAULT)
        }
    }
}
