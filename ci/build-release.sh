#!/bin/sh

set -eux

case $(uname) in
	Linux) target=x86_64-unknown-linux-musl;;
	Darwin) target=x86_64-apple-darwin;;
	*) echo "Unknown target $(uname)";;
esac

version=$(git describe --tags)

curl -LSfs https://japaric.github.io/trust/install.sh |
    sh -s -- --force --git rust-embedded/cross --tag v0.2.1 --target $target
command -v cross || PATH=~/.cargo/bin:$PATH

rustup target add $target
cross build --target $target --release
cross test  --target $target --release

src=$PWD
stage=$(mktemp -d)

cp target/$target/release/kak-lsp $stage
cp kak-lsp.toml $stage
cp README.asciidoc $stage
cp COPYING $stage
cp MIT $stage
cp UNLICENSE $stage

cd $stage
tar czf $src/kakoune-lsp-$version-$target.tar.gz *
cd $src

rm -rf $stage
