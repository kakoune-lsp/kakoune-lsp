## Unreleased

Breaking changes:
- The configuration syntax for `semantic_tokens` has changed. See the updated `kak-lsp.toml` for an example (#488).
- Snippet support has been disabled by default, as a workaround for conflicts with Kakoune's built-in completion (#282).
- `lsp-show-message`, which handles `window/showMessage` requests from the server has been removed. See below for the replacement.

Additions:
- `lsp-show-message` has been replaced by four separate commands `lsp-show-message-{error,warning,info,log}`.
  The new default implementations log the given messages from the language server to the debug buffer. Important messages are shown in `%opt{toolsclient}`.
- `lsp-code-actions` use the `menu` command to select an action interactively. The new command `lsp-show-code-actions` can be overridden to customize this behavior (#367).

Bug fixes:
- Fix renaming of Rust lifetimes (#474).

## 10.0.0 - 2021-06-03

This is a small release with a useful fix to `lsp-rename` (#481).

Breaking changes:
- The semantic highlighting feature has been removed. It is superseded by semantic tokens which are officially part of LSP 3.16.
- Buffers `*goto*`, `*symbols*`, `*diagnostics*` are given the `lsp-goto` filetype instead of `grep` or `make` (#454).
- `ocamllsp` replaces the discontinued `ocaml-language-server` as default language server for OCaml (#471).

Additions:
- `lsp-rename-prompt` is added to to the `lsp` user mode, mapped to `R` (#441).
- Default configuration for CSS variants "less" and "scss" (#473).
- `kak-lsp` sends the configured offset encoding to the language server (see https://clangd.llvm.org/extensions.html#utf-8-offsets), which still defaults to `utf-16`.
- `lua-language-server` was added as default language server for Lua.

Bug fixes:
- Fix edits (by `lsp-rename` and friends) to files that were not opened as Kakoune buffers (#481).
- `lsp-show-{diagnostics,goto-choices,document-symbols}` no longer `cd` to the project root (#454).
- Fix error when the documentation part of a completion item starts with a dash (`-`) (#460).
- Fix completions of non-ASCII characters for some textEdit completions (#455).
- Fix `lsp-rename` with `pyright` (#468).
- Treat snippets containing `<` literally, instead of as Kakoune key names (#470)
- Nested entries in `lsp_server_initialization_options` like `a.b=1` are sent to language servers as `{"a":{"b":1}}` instead of `{"a.b":1}`, matching the treatment of `lsp_server_configuration` (#480).
- Fix a case where `lua-language-server` would hang (#479) because `kak-lsp` didn't support `workspace/configuration`; basic support has been added.

For release notes on v9.0.0 and older see <https://github.com/kak-lsp/kak-lsp/releases>.
