#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat > main.go << EOF
package main

import "os"

func main() {}
EOF

test_tmux_kak_start 'edit main.go'

test_sleep
test_tmux send-keys ':lsp-code-actions-sync source.organizeImports' Enter
test_sleep
test_tmux capture-pane -p | sed 3q
# CHECK: package main
# CHECK:
# CHECK: func main() {}

test_tmux send-keys ':lsp-code-actions-sync source.organizeImports' Enter
test_sleep
test_tmux capture-pane -p | sed -n '$p'
# CHECK: lsp-code-actions: no matching action available{{.*}} 1 sel - client0@[session]
