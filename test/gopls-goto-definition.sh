#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat > main.go << EOF
package main

func foo() {}
func bar() { foo(); }
EOF

$tmux new-session -d -x 80 -y 6 kak -e "$kak_startup_commands; lsp-enable" main.go
$tmux resize-window -x 80 -y 6 ||: # Workaround for macOS.
sleep "$jiffy"
$tmux send-keys gj / foo Enter gd
sleep "$jiffy"
$tmux send-keys 'i%()' Escape

$tmux capture-pane -p
# CHECK: package main
# CHECK:
# CHECK: func %()foo() {}
# CHECK: func bar() { foo(); }
# CHECK: ~
# CHECK: main.go 3:9 [+] 1 sel - client0@[{{\d+}}]
