## Unreleased

Breaking changes:
- Buffers `*goto*`, `*symbols*`, `*diagnostics*` are given the `lsp-goto` filetype instead of `grep` or `make` (#454).

Additions:
- `lsp-rename-prompt` is added to to the `lsp` user mode, mapped to `R` (#441).

Bug fixes:
- `lsp-show-{diagnostics,goto-choices,document-symbols}` no longer `cd` to the project root (#454).
- Fix error when the documentation part of a completion item starts with a dash (`-`) (#460).
- Fix completions of non-ASCII characters when using textEdit (which is not only partially supported) (#455).

For release notes on v9.0.0 and older see <https://github.com/kak-lsp/kak-lsp/releases>.
