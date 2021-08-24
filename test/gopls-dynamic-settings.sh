#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat > main.go << EOF
package main

func format_me() {
	println("spurious blank line")

}
EOF

session=session
$tmux new-session -d -x 80 -y 7 kak -s "$session" -e "$kak_startup_commands; lsp-enable" main.go
$tmux resize-window -x 80 -y 7 ||: # Workaround for macOS.
sleep "$jiffy"

$tmux send-keys h,lf
sleep "$jiffy"

$tmux capture-pane -p
# CHECK: package main
# CHECK:
# CHECK: func format_me() {
# CHECK: 	println("spurious blank line")
# CHECK:
# CHECK: }
# CHECK:
# CHECK: main.go 1:1  1 sel - client0@[session]

echo '
set global lsp_config %{
	[language.go.settings.gopls]
	"formatting.gofumpt" = true
}' | kak -p $session
sleep "$jiffy"

$tmux send-keys ,lf # :lsp-formatting
sleep "$jiffy"

$tmux capture-pane -p
# CHECK: package main
# CHECK:
# CHECK: func format_me() {
# CHECK: 	println("spurious blank line")
# CHECK: }
# CHECK: ~
# CHECK: main.go 1:1 [+] 1 sel - client0@[session]
