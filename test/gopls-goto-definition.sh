#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat > main.go << EOF
package main

func foo() {}
func bar() { foo(); }
EOF

test_tmux_kak_start 'edit main.go'
test_tmux send-keys gj / foo Enter gd
test_sleep
test_tmux send-keys 'i%()' Escape

test_tmux capture-pane -p
# CHECK: package main
# CHECK:
# CHECK: func %()foo() {}
# CHECK: func bar() { foo(); }
# CHECK: ~
# CHECK: ~
# CHECK: {{(foo )?}} main.go 3:9 [+] 1 sel - client0@[session]
