# Change to a new HOME and sets $tmux and $kak_startup_commands.

set -e

# This test requires tmux.
command -v tmux >/dev/null

# Export a fresh HOME, so Kakoune is run without user configuration.
HOME=$(mktemp -d)
cd "$HOME"
export TMPDIR=$HOME # Avoid interfering with other kak-lsp processes.

# Isolated tmux.
tmux="tmux -S $HOME/.tmux-socket -f $HOME/.tmux.conf"

kak_startup_commands='
	evaluate-commands %sh{kak-lsp --kakoune -s $kak_session}
	map global user l %{: enter-user-mode lsp<ret>}
	# Enable logging since this is only for testing.
	set-option global lsp_cmd "kak-lsp -s %val{session} -vvv --log ./log"
'

jiffy=.3
if test -n "$CI"; then
	jiffy=10
fi


cat > "$HOME"/.tmux.conf << EOF
# Pass escape through with little delay, as suggested by the Kakoune FAQ.
set -sg escape-time 25
EOF

cleanup() {
	$tmux kill-server ||:
	# Language servers might still be running, so ignore errors for now.
	rm -rf "$HOME" >/dev/null 2>&1 ||:
}
trap cleanup EXIT
