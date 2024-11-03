#!/bin/sh

set -eux

case $(uname) in
	Linux) set -- x86_64-unknown-linux-musl ;;
	Darwin) set -- aarch64-apple-darwin x86_64-apple-darwin ;;
	*) echo "Unknown target $(uname)"; exit 1 ;;
esac

for target
do {
	version=$(git describe --tags)

	cargo=cargo
	if [ "$target" != aarch64-apple-darwin ]; then
		curl -LSfs https://japaric.github.io/trust/install.sh |
		    sh -s -- --force --git rust-embedded/cross --tag v0.2.1 --target $target
		command -v cross || PATH=~/.cargo/bin:$PATH
		rustup target add $target
		cargo=cross
	fi
	$cargo build --target $target --release
	$cargo test  --target $target --release

	src=$PWD
	stage=$(mktemp -d)

	cp target/$target/release/kak-lsp $stage
	cp README.asciidoc $stage
	cp COPYING $stage
	cp MIT $stage
	cp UNLICENSE $stage

	cd $stage
	tar czf $src/kakoune-lsp-$version-$target.tar.gz *
	cd $src

	rm -rf $stage
} done
