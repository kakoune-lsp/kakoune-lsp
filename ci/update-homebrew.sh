#!/bin/sh

set -ex

tag=$(git describe)
tag=${tag%%-*}
version=${tag#v}
url=https://github.com/kakoune-lsp/kakoune-lsp/releases/download/$tag/kakoune-lsp-$tag-x86_64-apple-darwin.tar.gz
curl -O -L "$url"
sha=$(sha256sum "kakoune-lsp-$tag-x86_64-apple-darwin.tar.gz")
sha=${sha%% *}

cd ../homebrew-kakoune-lsp/
sed 4c"  url \"$url\"" -i Formula/kakoune-lsp.rb
sed 5c"  sha256 \"$sha\"" -i Formula/kakoune-lsp.rb
sed 6c"  version \"$version\"" -i Formula/kakoune-lsp.rb
sed '4,6s/^/  /' -i Formula/kakoune-lsp.rb

git diff
read

git commit -am "$tag"
git push
