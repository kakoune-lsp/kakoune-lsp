#!/bin/sh

# REQUIRES: command -v haskell-language-server-wrapper

. test/lib.sh

cat > main.hs << EOF
triple l = l ++ l
EOF

test_tmux_kak_start 'edit main.hs'

test_sleep_until 'test_tmux capture-pane -p | grep -F ">triple l = l ++ l"'
# CHECK: >triple l = l ++ l

test_tmux send-keys :lsp-code-lens Enter

test_sleep_until 'test_tmux capture-pane -p | grep "triple :: \[a] -> \[a]"'
# CHECK: {{.*}}triple :: [a] -> [a]{{.*}}

test_tmux send-keys Enter

test_sleep_until 'test_tmux capture-pane -p | grep "^ triple :: \[a] -> \[a]"' >/dev/null
test_tmux capture-pane -p | head -2
# CHECK: triple :: [a] -> [a]
# CHECK: triple l = l ++ l
