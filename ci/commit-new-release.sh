#!/bin/sh

set -eu

new=$1 # Pass the version numbers, like "1.2.3"

if ! git diff HEAD --quiet; then
	echo "$0: index and worktree must be clean"
	exit 1
fi

old=$(git describe --tags | sed 's/^v//;s/-.*//')

set -x

sed -i "s/v$old/v$new/" README.asciidoc
sed -i "0,/version/ s/$old-snapshot/$new/" Cargo.toml
cargo check # update Cargo.lock
git commit -am "v$new"
git tag "v$new"
sed -i "0,/version/ s/$new/$new-snapshot/" Cargo.toml
cargo check # update Cargo.lock
git commit -am 'start new cycle'

cat <<EOF

Release checklist:
- Prepare release notes
- Push the tag v$new
- Wait for the CI to create the release draft with release artifacts, then
  edit release notes and make the release at https://github.com/kak-lsp/kak-lsp/releases
- Update the Homebrew formula at https://github.com/kak-lsp/homebrew-kak-lsp
EOF
