#!/bin/sh

set -eu

new=$1 # Pass the version numbers, like "1.2.3"

if ! git diff HEAD --quiet; then
	echo "$0: index and worktree must be clean"
	exit 1
fi

untracked=$(git ls-files --exclude-standard --others)
if [ -n "$untracked" ]; then
	echo "$0: must not have untracked files (cargo limitation), found: $untracked"
	exit 1
fi

old=$(git describe --tags | sed 's/^v//;s/-.*//')

if git tag | grep -qxF "v$new"; then
	echo "tag v$new already exists"
	exit 1
fi

set -x

sed -i "s/v$old/v$new/g" README.asciidoc
sed -i "0,/version/ s/$old-snapshot/$new/" Cargo.toml
sed -i "1s/Unreleased/$new - $(date --iso)/" CHANGELOG.md
cargo check # update Cargo.lock
git commit -am "v$new"
git tag "v$new" --message="$(ci/latest-changelog.sh)"
cargo publish

sed -i "0,/version/ s/$new/$new-snapshot/" Cargo.toml
cargo check # update Cargo.lock
sed -i 1i'## Unreleased\n' CHANGELOG.md
git commit -am 'start new cycle'

git push origin v$new

# Update homebrew package.
url=https://github.com/kakoune-lsp/kakoune-lsp/releases/download/v$new/kakoune-lsp-v$new-x86_64-apple-darwin.tar.gz
archive=kakoune-lsp-v$new-x86_64-apple-darwin.tar.gz
while true
do
	curl -O -L "$url"
	if file --brief --mime "$archive" | grep -q application/gzip; then
		break
	fi
	sleep 60
done
sha=$(sha256sum "$archive")
sha=${sha%% *}
rm "$archive"
(
	cd ../homebrew-kakoune-lsp/
	sed 4c"  url \"$url\"" -i Formula/kakoune-lsp.rb
	sed 5c"  sha256 \"$sha\"" -i Formula/kakoune-lsp.rb
	sed 6c"  version \"$new\"" -i Formula/kakoune-lsp.rb
	sed '4,6s/^/  /' -i Formula/kakoune-lsp.rb

	git commit -am "v$new"
	git push
)

git push origin
