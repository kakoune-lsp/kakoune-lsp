#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat >> .config/kak/kakrc << EOF
hook global BufWritePre .*[.]go %{
	try %{ lsp-code-actions-sync source.organizeImports }
	lsp-formatting-sync
}
EOF

cat > main.go << EOF
package main

import "os"

	func main (){}
EOF

test_tmux_kak_start 'edit main.go'

test_sleep
test_tmux send-keys ':w' Enter
test_sleep
test_tmux capture-pane -p | sed 3q
# CHECK: package main
# CHECK:
# CHECK: func main() {}
