#!/bin/sh

# REQUIRES: command -v typescript-language-server

. test/lib.sh

cat >> .config/kak/kakrc << EOF
set-option global lsp_auto_show_code_actions true

hook global BufSetOption filetype=typescript %{
	set-option buffer lsp_servers %{
		[language_server.typescript-language-server]
		command = "typescript-language-server"
		args = ["--stdio"]
	}
	lsp-find-root *.ts
}
EOF

cat > main.ts << EOF
class MyClass {
	doSomething() {
		return false;
	}
}
EOF

test_tmux_kak_start 'edit main.ts'

test_tmux send-keys /doSomething Enter
test_sleep_until 'test_tmux capture-pane -p | grep -Eo "💡|\[A\]"'
# CHECK: {{💡|\[A\]}}

test_tmux send-keys :lsp-code-actions Enter
test_sleep_until 'test_tmux capture-pane -p | grep -o "Infer function return type"'
# CHECK: Infer function return type

test_tmux send-keys Enter
test_sleep_until 'test_tmux capture-pane -p | grep -F "doSomething(): boolean {"'
# CHECK: doSomething(): boolean {
