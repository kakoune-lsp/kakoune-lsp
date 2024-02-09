#!/bin/sh

# REQUIRES: command -v rust-analyzer

user_home=$HOME

. test/lib.sh

cat > .config/kak-lsp/kak-lsp.toml << EOF
[language.rust]
filetypes = ["rust"]
roots = ["Cargo.toml"]
command = "sh"
args = ["-c", "RUSTUP_HOME=$user_home/.rustup rust-analyzer"]
EOF

cat >> .config/kak/kakrc << EOF
set-option global lsp_auto_show_code_actions true
EOF

cat > main.rs << EOF
enum Test {
    Foo,
    Bar,
}
fn main() {
    let foo: Test = Test::Foo;
    match foo {
        Test::Foo => println!("foo"),
        _ => (),
    }
}
EOF

cat >> .config/kak/kakrc << 'EOF'
declare-option range-specs lsp_code_actions_hint
add-highlighter global/lsp_code_actions_hint replace-ranges lsp_code_actions_hint
define-command -override -hidden lsp-show-code-actions -params 1.. %{
	evaluate-commands -draft %{
		execute-keys <a-l><semicolon>
		set-option buffer lsp_code_actions_hint %val{timestamp} "%val{cursor_line}.%sh{echo $((kak_cursor_column+1))}+0| 💡"
	}
}
define-command -override -hidden lsp-hide-code-actions %{
	set-option buffer lsp_code_actions_hint %val{timestamp}
}
EOF

test_tmux_kak_start 'edit main.rs'
test_tmux send-keys j/foo Enter vtj
test_sleep_until 'test_tmux capture-pane -p | grep 💡'
# CHECK:      match foo { 💡
