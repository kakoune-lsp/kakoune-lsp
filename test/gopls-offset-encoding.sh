#!/bin/sh

# REQUIRES: command -v gopls

. test/lib.sh

cat > main.go << EOF
package main

func main() {
	ASCII()
	BMP()
	BeyondBMP()
}

func /*__________*/ ASCII() {
}
func /*åååååååååå*/ BMP() {
}
func /*🐣🐣🐣🐣🐣*/ BeyondBMP() {
}
EOF

test_tmux_kak_start 'edit main.go'

# This is kak-lsp uses Unicode code points, while gopls uses UTF-16.

# This means that everything in the Basic Multilingual Plane works.
test_tmux send-keys / BMP Enter gd
test_sleep
test_tmux send-keys 'i%()' Escape
test_sleep_until 'test_tmux capture-pane -p | grep -F "%()"'
#CHECK: func /*åååååååååå*/ %()BMP() {

# Anything beyond BMP will be off.
# The chicken symbolize the chicken-and-egg nature of the problem.
test_tmux send-keys Escape u gk / BeyondBMP Enter gd
test_sleep
test_tmux send-keys 'i%()' Escape
test_sleep_until 'test_tmux capture-pane -p | grep -F "%()"'
#CHECK: func /*🐣🐣🐣🐣🐣*/ Beyon%()dBMP() {
