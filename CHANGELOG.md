## Unreleased

Additions:
- Support multiple language servers per filetype (#17).
- The `kak-lsp.toml` format for specifying language servers has changed. The old format is still supported (#686).
- `lsp-goto-document-symbol` learned to show a preview of the jump target.
- Default configuration for PureScript and Scala.

Fixes:
- A regression broke resolving completion item documentation when cycling through completion candidates, which has been fixed (#674).
- New command `lsp-declaration`, implementing `textDocument/declaration`.

## 14.2.0 - 2023-02-13

Additions:
- Default configuration for CMake.
- If there are no code actions in the main selection, `lsp-code-actions` will show code actions from anywhere on a selected line, to make it easier to perform quick-fixes.
- If requested by the language server, kak-lsp will recursively watch the entire root directory for file changes and forward them to the server via `workspace/didChangeWatchedFiles` (#649).
- kak-lsp now asks the server to cancel stale requests (#666).
- `lsp-did-change` is async again, which can improve performance (#667).
- kak-lsp is published to crates.io for easy installation (#660).

Fixes:
- Fix race conditions when spinning up a new server in "kak-lsp --request" (#654), and in "lsp-hover-buffer" (#664).
- Send inlay hints and semantic tokens only after buffer changes (#663).

## 14.1.0 - 2022-10-26

Additions:
- Default language server for protobuf.
- Added support for `codeAction/resolve`, which allows to use code actions sent by Deno for example.
- The recommended mappings have been augmented by new command `lsp-diagnostic-object` to jump to next/previous diagnostics.
- `lsp-auto-signature-help-enable` now shows an info box by default, and formats the active parameter in a bold font.
- `lsp-definition` and friends now select the symbol name instead of merely placing the cursor at symbol start. Same for `lsp-find-error`.
- `lsp-highlight-references` now selects all references in the current buffer.
- New `lsp-inlay-code-lenses-enable` command allows to render code lenses as virtual text (#623).
- The support for `filterText` in completions no longer depends on an out-of-tree Kakoune feature.

Fixes:
- Fix lags due to `rust-analyzer` sending a ton of progress reports.
- `lsp-rename` will now write hidden buffers that are affected by the rename, to give the language server and other external tools a more consistent view of affected files.
- Suppress "language server not initialized" errors that originate from hooks.
- Fix a glitch when a line has both a code lens and an inline diagnostic.
- When talking to servers that don't support UTF-8 byte-offsets, `kak-lsp` now adheres to the LSP specification by treating column-offsets as UTF-16 Code Units instead of Unicode Code Points.

## 14.0.0 - 2022-08-29

This is a small bug fix release but it also includes updates to the default config.

Breaking changes:
- [`typescript-language-server`](https://github.com/typescript-language-server/typescript-language-server) replaces `flow` as default language server for JavaScript.
- [`crystalline`](https://github.com/elbywan/crystalline) replaces `scry` as default language server for Crystal.
- The user mode mapping for `s` now runs `lsp-goto-document-symbol` instead of `lsp-signature-help`. The old mapping (`<c-o>`) has been removed.

Fixes:
- `lsp-find-error` and code lenses were broken in 13.0.0, which has been fixed.
- The server-specific configuration for the `solargraph` Ruby language server has been fixed.
- The new command `lsp-goto-document-symbol` (which replaced the old `<c-o>` binding) can now handle nested symbols.

## 13.0.0 - 2022-08-11

Here are some notable changes. See the git log for a full list of changes.

Breaking changes:
- Completion snippet support is now enabled again in the default config.
- The default object mode mappings have been removed. Users are expected to add their preferred mappings. The README now has a section with recommended mappings.

Additions:
- Default configuration for Elvish and Clojure.
- On macOS, the config file is now read from `$XDG_CONFIG_HOME/kak-lsp/kak-lsp.toml`. The old location (`~/Library/Preferences/kak-lsp/kak-lsp.toml`) is used as a fallback (#293).
- Added shim support for `workspace/WorkspaceFolders`, which fixes interaction with `bash-language-server`.
- Some errors are shown directly in the editor, unless the error was triggered by a hook.
- `lsp-document-symbol` now indents nested symbols.

Fixes:
- Completions with snippets now interact properly with Kakoune's completion engine (#616).
- Fixed default configuration for HTML/CSS/JSON following the upstream renaming of the binaries.
- `lsp-enable-window` no longer adds a redundant global `NormalKey` hook (introduced in 12.1.0)
- kak-lsp now avoids sending unsupported requests to the server.

## 12.2.1 - 2022-05-08

Fixes:
- `lsp-code-lens` failed to run code lenses which has been fixed.

## 12.2.0 - 2022-05-03

Deprecations:
- `rust-analyzer-inlay-hints` is deprecated in favor of `lsp-inlay-hints-enable`. Inlay hints now requires `rust-analyzer` version >= 2022-04-18. (#602, #613).

Additions:
- Support LSP Code Lenses (#490).
- Support the new `textDocument/inlayHint` request from LSP v3.17 (#600).
- New command `lsp-inlay-hints-enable` simplifies inlay hints configuration (#602).
- Completion item documentation can now be resolved lazily if required by the language server (6bee0a13).
- `lsp-hover` now shows diagnostics from anywhere on a line touched by the main selection, instead of just diagnostics whose range contains the cursor (5461c141).
- `lsp_hover_max_lines` now defaults to 20 which limits the size of the info box spawned by `lsp-hover` (#615).

Fixes:
- `lsp-formatting` with `pylsp` now preserves cursor positions again (regressed in 12.1.0).
- Inlay diagnostics are now disabled in insert mode, to avoid diagnotsics jumping around while typing (#605).
- The default config for `typescript-language-server` now supports TSX files (#211).
- `lsp-object` (`]a` etc.) can now jump past the symbols sent by `pylsp`.
- `texlab-forward-search`'s SyncTeX inverse search now works in more scenarios (#603).
- Support server specific configuration servers like `pylsp` that don't use `initializationOptions` for configuration (#611).

## 12.1.0 - 2022-03-28

Additions:
- New mappings for Kakoune's object mode allow to select adjacent/surrounding symbols like functions or types (#598).
- New mapping for the `lsp` user mode, `<c-o>`, spawns a menu to jump to buffer symbol (#584).
- `lsp-formatting` used to spawn several shell processes for each modification, which caused noticable delays when formatting many lines. This has been remedied by avoiding shell calls (88f52f0c).
- kak-lsp is now compatible with the proposed changes to P and <a-x> (see Kakoune's `breaking-cleanups` branch).
- The documentation now communicates that kak-lsp appends to the global modeline at load time (eb54d378).
- New experimental command `lsp-connect` allows to handle certain LSP responses messages with a custom command (#584).

Bug fixes:
- `lsp_auto_show_code_actions` has been fixed to actually hide the lightbulb when no code action is available (76cff5f2).
- Loading kak-lsp no longer leaves around a scratch buffer (#593).
- Code actions are now offered for the main selection's range, instead of just the cursor position. This unlocks an "extract to function" refactoring from rust-analyzer. (#594).
- The *-sync commands now automatically restart the server instead of showing an error if the server is down (b54ec807).

## 12.0.1 - 2022-01-29

Bug fixes:
- Fix regression in 12.0.0 where kak-lsp would panic when applying text edits that span until the buffer end, such as from `lsp-formatting` with zls (#589).
- Diagnostics no longer break when a diagnostic's message contains `%` (#590).

## 12.0.0 - 2022-01-26

Breaking changes:
- [`python-lsp-server`](https://github.com/python-lsp/python-lsp-server) replaces [python-language-server](https://github.com/palantir/python-language-server) as default language server for Python.
- `rust-analyzer` replaces `rls` as default language server for Rust (#578).

Additions:
- `lsp-handle-progress` gained a default implementation that shows an hourglass (⌛) in the modeline when a language server is busy (f8a8cf8).
- The code action menu now provides fuzzy (subsequence) filtering but no longer provides `<c-n>`/`<c-p>` due to implementation reasons (b4ee2a3).
- When accepting a completion, any `additionalTextEdits` are now applied. For example, rust-analyzer adds import statements this way (2637b26).
- New command `lsp-code-action-sync` is the synchronous variant of `lsp-code-action`, suitable for use in `BufWritePre` hooks (#582).
- The default configuration for `semantic_tokens` now uses a more condensed syntax (#587).
- Some systems' C libraries ship old Unicode databases, but terminals often bundle newer versions. To avoid graphical glitches, emoji are only used if they are assigned display width 2 by the C library's `wcwidth(3)` (73c2d1a, c3aeb0d).
- The [clangd protocol extension for offset encoding negotiation](https://clangd.llvm.org/extensions.html#utf-8-offsets) is now supported, which means kak-lsp now uses UTF-8 offsets if the server supports them (#485).

Bug fixes:
- `lsp-formatting-sync` now works again with `texlab` instead of blocking forever (regressed in 11.0.0) (a12831a).
- Certain text edits (such as some code actions from `rust-analyzer`) were not applied correctly, which has been fixed (35ba8de).

## 11.1.0 - 2021-12-08

Additions:
- Default configuration for Erlang (#548), R (#555) and Racket (#568).
- Goto/get hover info of next/previous function with new `lsp-next-function`/`lsp-hover-next-function` (#557).
- Allow to show hover info in the `*hover*` buffer (instead of the info box) with new `lsp-hover-buffer` (#564, #257).
- Show lightbulb in modeline when code actions are available and `lsp_auto_show_code_actions` is set (#538).
- Run specific code actions with new `lsp-code-action` (#566).
- Support LSP's Call Hierarchy, to show callers/callees of the function at cursor with `lsp-incoming-calls`/`lsp-outgoing-calls` (#554).
- Support LSP's Selection Range, to quickly select interesting ranges around cursors with `lsp-selection-range` (#288).
- Create just one undo entry for sequences of text edits (like from `lsp-code-actions` or `lsp-formatting`) (#533).
- Set new environment variable `KAK_LSP_FORCE_PROJECT_ROOT` to use the same project root even for files outside a project, to reuse a single language server (#542).
- Support server progress notifications via `$/progress`, removing support for the non-standard `window/progress` (#545).
- Support for `filterText` in completions, which depends on a proposed Kakoune feature (#551).
- Allow multiple characters and spaces in diagnostic gutter via `lsp_diagnostic_line_error_sign` and friends (#571).
- Support LaTeX language server `texlab`'s custom commands with `texlab-build` and `texlab-forward-search` (SyncTeX support) (#573).

Bug fixes:
- Honor `extra_word_chars` for completions, fixing completion of Lisps (#568).
- Fix go-to-definition for files containing invalid UTF-8 (#535).
- Fix default server-specific configuration for `pyls` (regressed in 11.0.0).
- Use the LineNumbers face for the flag-lines highlighter that shows diagnostics, to work better with non-default backgrounds (#524).
- Fix applying sequences of text edits, like from `lsp-code-actions` or `lsp-formatting` (#527).
  - Also, do not drop trailing newline from text edits (e9af1aa).
- Quoting/escaping fixes for diagnostics and hover info.

## 11.0.1 - 2021-09-17

This is mostly a bug fix release but also includes Markdown rendering and enhanced inlay diagnostics.

Breaking changes:
- The face `LineFlagErrors` has been renamed to `LineFlagError`, for consistency with other faces (#516).

Bug fixes:
- Fix example server configuration for `lua-language-server`.
- Fix completion of tokens containing non-word-characters, such as Ruby attributes, Ruby symbols and Rust lifetimes (#378, #510).
- `lsp-highlight-references` clears highlights on failure, improving the behavior when `%opt{lsp_auto_highlight_references}` is set to true (#457).
- Fix jumping to locations when Kakoune working directory is different from project root (#517).
- Fix jumping to locations with absolute paths (#519).
- Diagnostics of level "info" and "hint" are no longer shown as "warning", and are given distinct faces (#516).
  - `find-next-error` will skip over "info" and "hint" diagnostics.
- Fix adjacent insertion text edits, for example as sent by rust-analyzer as code actions (#512).
- The default `kak-lsp.toml` recognizes `.git` and `.hg` as project root markers for C/C++ files. This makes `lsp-references` work better out-of-the-box for projects like Kakoune that don't need a `compile_commands.json`.

Additions:
- Render Markdown from hover and from completions in info box. You can set custom faces to highlight different syntax elements (#73, #513).
  - Some servers like `pyls` send that info in plaintext but label it as Markdown. Work around this with a new configuration option `workaround_server_sends_plaintext_labeled_as_markdown` in the default `kak-lsp.toml` to force plaintext rendering.
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
