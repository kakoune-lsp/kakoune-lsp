#!/bin/sh

# REQUIRES: command -v clangd

. test/lib.sh

cat >> .config/kak/kakrc << EOF
set-option global lsp_diagnostic_line_error_sign   ' X'
set-option global lsp_diagnostic_line_hint_sign    '¿ '
set-option global lsp_diagnostic_line_info_sign    '¡ '
set-option global lsp_diagnostic_line_warning_sign 'W '
EOF

cat > main.c << EOF
void main(int argc, char** argv) {}
syntax error
EOF

test_tmux_kak_start 'edit main.c'
test_tmux capture-pane -p | sed 2q
# CHECK: {{W }}void main(int argc, char** argv) {}
# CHECK: {{ X}}syntax error

test_tmux send-keys %:comment-line Enter
test_sleep
test_tmux capture-pane -p | sed 2q
# CHECK: {{  }}// void main(int argc, char** argv) {}
# CHECK: {{  }}// syntax error
