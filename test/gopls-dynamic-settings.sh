#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat > main.go << EOF
package main

func format_me() {
	println("spurious blank line")

}
EOF

test_tmux_kak_start 'edit main.go'

test_tmux send-keys h:lsp-formatting Enter
test_sleep
test_tmux capture-pane -p
# CHECK: package main
# CHECK:
# CHECK: func format_me() {
# CHECK: 	println("spurious blank line")
# CHECK:
# CHECK: }
# CHECK:
# CHECK: main.go 1:1  1 sel - client0@[session]

echo '
set global lsp_config %{
	[language_server.gopls.settings.gopls]
	"formatting.gofumpt" = true
}' | kak -p "$test_kak_session"
test_sleep
test_tmux send-keys :lsp-formatting Enter
test_sleep
test_tmux capture-pane -p
# CHECK: package main
# CHECK:
# CHECK: func format_me() {
# CHECK: 	println("spurious blank line")
# CHECK: }
# CHECK: ~
# CHECK: main.go 1:1 [+] 1 sel - client0@[session]
