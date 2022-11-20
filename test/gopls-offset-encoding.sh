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

# Check that characters in the Basic Multilingual Plane work.
test_tmux send-keys / BMP Enter gd
test_sleep
test_tmux send-keys 'i/**/' Escape
test_sleep_until 'test_tmux capture-pane -p | grep -F "/**/"'
#CHECK: func /*åååååååååå*/ /**/BMP() {

# Check that characters outside the BMP work.
test_tmux send-keys u gk / BeyondBMP Enter gd
test_sleep
test_tmux send-keys 'i/**/' Escape
test_sleep_until 'test_tmux capture-pane -p | grep -F "/**/"'
#CHECK: func /*🐣🐣🐣🐣🐣*/ /**/BeyondBMP() {
