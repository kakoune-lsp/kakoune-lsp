# Change to a new HOME and define some test_* functions and variables.

set -e

if [ -z "$KAK_LSP_TEST_INSTALLED" ]; then
	for build in debug release
	do
		if command -v "$PWD/target/$build/kak-lsp" >/dev/null; then
			PATH="$PWD/target/$build:$PATH"
			break
		fi
	done
fi

command -v tmux >/dev/null

# Export a fresh HOME, so Kakoune runs without user configuration.
HOME=$(mktemp -d)
cd "$HOME"
export TMPDIR=$HOME # Avoid interfering with other kakoune-lsp processes.
env=$(env)
if printf %s "$env" | grep -q ^XDG_CONFIG_HOME=; then
	XDG_CONFIG_HOME=$HOME/.config
fi
if printf %s "$env" | grep -q ^XDG_RUNTIME_DIR=; then
	XDG_RUNTIME_DIR=$HOME/xdg_runtime_dir
	mkdir -m 700 "$XDG_RUNTIME_DIR"
fi

test_kak_session=session
mkdir .config
mkdir .config/kak-lsp
mkdir .config/kak
cat > .config/kak/kakrc << 'EOF'
evaluate-commands %sh{kak-lsp --kakoune -s $kak_session}
map global user l %{: enter-user-mode lsp<ret>}
# Enable logging since this is only for testing.
set-option global lsp_cmd "%opt{lsp_cmd} -vvvv --log ./log"
hook global -once WinDisplay .* lsp-enable

EOF

test_tmux_kak_start() {
	# If we directly run kak, then "lsp-stop" will not send the exit notification.
	test_tmux new-session -d -x 80 -y 7 /bin/sh
	test_tmux resize-window -x 80 -y 7 ||: # Workaround for macOS.
	autoload='
		find -L "$kak_runtime/autoload" -type f -name "*\.kak" |
		sed "s/.*/try %{ source & } catch %{ echo -debug Autoload: could not load & }/"
	'
	load_default_config="
		evaluate-commands %sh{$autoload}
		source \"%val{config}/kakrc\"
	"
	test_tmux send-keys "kak -s $test_kak_session -n -e '$load_default_config; $@'" Enter
	test_sleep
}

cat > .tmux.conf << 'EOF'
# Pass escape through with less delay, as suggested by the Kakoune FAQ.
set -sg escape-time 25
EOF

test_tmux() {
	# tmux can't handle session sockets in paths that are too long, and macOS has a very
	# long $TMPDIR, so use a relative path. Make sure no one calls us from a different dir.
	if [ "$PWD" != "$HOME" ]; then
		echo "error: test_tmux must always be run from the same directory." >&2
		return 1
	fi
	tmux -S .tmux-socket -f .tmux.conf "$@"
}

test_sleep()
{
	if [ -n "$CI" ]; then
		sleep 10
	else
		sleep 1
	fi
}

test_sleep_until()
{
	i=0
	while [ $i -lt 100 ] && ! eval "$1" >/dev/null
	do
		sleep 1
		i=$((i + 1))
	done
	if [ $i -eq 100 ]; then
		printf %s\\n "timeout waiting for $1" >&2
		return 1
	else
		eval "$1"
	fi
}

test_cleanup() {
	echo kill! | kak -p "$test_kak_session"
	test_sleep_until "! kak -c $test_kak_session -ui dummy -e quit >/dev/null 2>&1"
	sleep .1
	test_tmux kill-server >/dev/null 2>&1
	sleep .1
	rm -rf "$HOME"
}
trap test_cleanup EXIT
