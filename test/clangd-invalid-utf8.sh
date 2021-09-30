#!/bin/sh

# REQUIRES: command -v clangd

. test/lib.sh

cat > main.c << EOF
#include "callee.c"
void caller() { callee(); }
EOF

cat > callee.c << EOF
void callee() {}
/* Invalid UTF-8 in comment: $(printf '\xff')
 */
EOF

$tmux new-session -d -x 80 -y 6 kak -e "$kak_startup_commands; lsp-enable" main.c
$tmux resize-window -x 80 -y 6 ||: # Workaround for macOS.
sleep "$jiffy"
$tmux send-keys / callee Enter gd
sleep "$jiffy"
$tmux send-keys 'i%()' Escape

$tmux capture-pane -p
# CHECK: %()void callee() {}
# CHECK: /* Invalid UTF-8 in comment:
# CHECK:  */
# CHECK: ~
# CHECK: ~
# CHECK: callee.c 1:4 [+] 1 sel - client0@[{{\d+}}]
