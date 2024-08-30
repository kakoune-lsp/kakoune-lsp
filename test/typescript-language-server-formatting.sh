#!/bin/sh

# REQUIRES: command -v typescript-language-server

. test/lib.sh

cat >> .config/kak/kakrc << EOF
set-option global lsp_auto_show_code_actions false

hook global BufSetOption filetype=typescript %{
	set-option buffer lsp_servers %{
		[language_server.typescript-language-server]
		args = ["--stdio"]
	}
	lsp-find-root main.ts
}
EOF

cat > main.ts << EOF
/**
 * Test class to format
 */
class MyClass {
	doSomething() { return false; } }
EOF

test_tmux_kak_start 'edit main.ts'

test_tmux send-keys :lsp-formatting-sync Enter
test_sleep_until 'test_tmux capture-pane -p | grep -qF [+]'
test_tmux capture-pane -p
# CHECK:  /**
# CHECK:   * Test class to format
# CHECK:   */
# CHECK:  class MyClass {
# CHECK:          doSomething() { return false; }
# CHECK:  }
# CHECK:                                        main.ts 1:1 [+] 1 sel - client0@[session]

# Repeated formatting gives no error.
test_tmux send-keys :lsp-formatting-sync Enter
test_sleep
test_tmux capture-pane -p
# CHECK:  /**
# CHECK:   * Test class to format
# CHECK:   */
# CHECK:  class MyClass {
# CHECK:          doSomething() { return false; }
# CHECK:  }
# CHECK:                                        main.ts 1:1 [+] 1 sel - client0@[session]
