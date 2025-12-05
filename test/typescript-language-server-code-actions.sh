#!/bin/sh

# REQUIRES: command -v typescript-language-server
# REQUIRES: false

. test/lib.sh

cat >> .config/kak/kakrc << 'EOF'
hook global BufSetOption filetype=typescript %{
	set-option buffer lsp_servers %{
		[typescript-language-server]
		root_globs = ['*.ts']
		command = "typescript-language-server"
		args = ["--stdio"]
	}
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
