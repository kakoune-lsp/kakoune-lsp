#!/bin/sh

set -ex

tag=$(git describe)
tag=${tag%%-*}
version=${tag#v}
url=https://github.com/kakoune-lsp/kakoune-lsp/releases/download/$tag/kakoune-lsp-$tag-x86_64-apple-darwin.tar.gz
archive=kakoune-lsp-$tag-x86_64-apple-darwin.tar.gz
while ! file --brief --mime "$archive" | grep -q application/gzip
do
	curl -O -L "$url"
	sleep 60
done
sha=$(sha256sum "$archive")
sha=${sha%% *}

cd ../homebrew-kakoune-lsp/
sed 4c"  url \"$url\"" -i Formula/kakoune-lsp.rb
sed 5c"  sha256 \"$sha\"" -i Formula/kakoune-lsp.rb
sed 6c"  version \"$version\"" -i Formula/kakoune-lsp.rb
sed '4,6s/^/  /' -i Formula/kakoune-lsp.rb

git commit -am "$tag"
git push
