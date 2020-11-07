#!/bin/sh

set -eux

target=${1:-x86_64-unknown-linux-musl}

version=$(git describe --tags)

if command -v gsort >/dev/null; then
    sort=gsort # for `sort --sort-version`, from brew's coreutils.
else
    sort=sort
fi

# This fetches latest stable release
tag=$(git ls-remote --tags --refs --exit-code https://github.com/rust-embedded/cross \
            | cut -d/ -f3 \
            | grep -E '^v[0-9.]+$' \
            | $sort --version-sort \
            | tail -n1)
curl -LSfs https://japaric.github.io/trust/install.sh |
    sh -s -- --force --git rust-embedded/cross --tag $tag --target $target
command -v cross || source ~/.cargo/env

# cross build --target $target --release TODO
# cross test  --target $target --release TODO

src=$PWD
stage=$(mktemp -d)

# cp target/$target/release/kak-lsp $stage TODO
cp kak-lsp.toml $stage
cp README.asciidoc $stage
cp COPYING $stage
cp MIT $stage
cp UNLICENSE $stage

cd $stage
tar czf $src/kak-lsp-$version-$target.tar.gz *
cd $src

rm -rf $stage
