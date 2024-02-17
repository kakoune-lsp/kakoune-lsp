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

test_tmux_kak_start 'edit main.c'
test_tmux send-keys / callee Enter gd
test_sleep
test_tmux send-keys 'i%()' Escape
test_tmux capture-pane -p
# CHECK: %()void callee() {}
# CHECK: /* Invalid UTF-8 in comment:
# CHECK:  */
# CHECK: ~
# CHECK: ~
# CHECK: ~
# CHECK: {{(callee )?}} callee.c 1:4 [+] 1 sel - client0@[session]
