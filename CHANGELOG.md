## Unreleased

Breaking changes:
- The face `LineFlagErrors` has been renamed to `LineFlagError`, for consistency with other faces (#516).

Bug fixes:
- Fix example server configuration for `lua-language-server`.
- Fix completion of tokens containing non-word characters, such as Ruby attributes, Ruby symbols and Rust lifetimes (#378, #510).
- `lsp-highlight-references` clears highlights on failure, improving the behavior when `%opt{lsp_auto_highlight_references}` is true (#457).
- Fix jumping to locations when Kakoune working directory is different from project root (#517).
- Fix jumping to locations with absolute paths (#519).
- Diagnostics of level "info" and "hint" are no longer shown as "warning", and are given distinct faces. Also, `find-next-error` will skip over "info" and "hint" diagnostics (#516).
- Fix adjacent insertion text edits, for example as sent by rust-analyzer as code actions (#512).
- The default `kak-lsp.toml` recognizes `.git` and `.hg` as project root markers for C/C++ files. This makes `lsp-references` work better out-of-the-box for projects like Kakoune that don't need a `compile_commands.json`.

Additions:
- Render Markdown from hover and from completions in info box. You can set custom faces to highlight different syntax elements (#73, #513).
- Multiple inlay diagnostics on a single line are coalesced (#515).

## 11.0.0 - 2021-09-01

Breaking changes:
- Two incompatible changes to the configuration format - see the updated `kak-lsp.toml` for examples. 
  - `semantic_tokens` syntax has changed, allowing for more fine-grained face configuration (#488)
  - `settings` replaces `initialization_options` for server-specific settings (#511)
- Snippet support has been disabled by default, as a workaround for conflicts with Kakoune's built-in completion (#282).
- `lsp-show-message`, which handles `window/showMessage` requests from the server has been removed. See below for the replacement.
- Hidden commands `lsp-next-match` and `lsp-previous-match` were removed in favor of `lsp-next-location` and `lsp-previous-location` (#466).
- `haskell-language-server` is the new default language server for Haskell, replacing `haskell-ide-engine`.

Additions:
- Finish support for `workspace/configuration` (#234).
- `%opt{lsp_config}` allows to set server-specific settings dynamically (#500).
- Default configuration for Julia (#502).
- `lsp-show-message` has been replaced by four separate commands `lsp-show-message-{error,warning,info,log}`.
  The new default implementations log the given messages from the language server to the debug buffer. Important messages are shown in `%opt{toolsclient}`.
- The new command `lsp-show-code-actions` can be overridden to customize the default menu behavior of `lsp-code-actions` (#367).
- New commands `lsp-{next,previous}-location` generalize `grep-next-match`, `lsp-next-match` and friends (#466).
- New option `lsp_location_format` to customize the "<file>:<line>"-style location patterns that `lsp-{next,previous}-location` can match (#466).

Bug fixes:
- Fix renaming of Rust lifetimes (#474).
- The suggested config for `rust-analyzer` was fixed for the case that `rustup` is installed but `rust-analyzer` is not installed via `rustup`.
- Fix spurious cursor movement on `lsp-rename` and `lsp-rename-prompt` (#504).
- Fix responses to `workspace/configuration` in case there are no initialization options set (#509).

Deprecations:
- `%opt{lsp_server_initialization_options}` and `%opt{lsp_server_configuration}` are deprecated in favor of setting `[language.<filetype>.settings]` in `%opt{lsp_config}`(#500).
- `lsp-{goto,symbols}-{next,previous}-match` are deprecated in favor of commands like `lsp-next-location *goto*` and similar (#466).

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
