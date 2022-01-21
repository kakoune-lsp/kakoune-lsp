#!/bin/sh

. test/lib.sh

# Some systems ship old versions of wcwidth(3) that return width 1 for emoji.  At the same time,
# some terminals ship a more modern wcwidth(). This discrepancy causes a rendering glitch:
# Kakoune sees width 1, so it thinks it can draw $COLUMNS emoji in one line, but the terminal
# might break the line earlier. Test our workaround (i.e. there are no rendering glitches).

cat >> .config/kak/kakrc << EOF
set-option global modelinefmt 💡⌛
EOF

cat > file << EOF
line 1
line 2
line 3
EOF

test_tmux_kak_start file
test_tmux capture-pane -p | sed 3q
# CHECK: line 1
# CHECK: line 2
# CHECK: line 3
