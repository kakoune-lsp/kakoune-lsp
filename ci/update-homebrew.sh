#!/bin/sh

set -ex

tag=$(git describe)
tag=${tag%%-*}
version=${tag#v}
url=https://github.com/kak-lsp/kak-lsp/releases/download/$tag/kak-lsp-$tag-x86_64-apple-darwin.tar.gz
curl -O -L "$url"
sha=$(sha256sum "kak-lsp-$tag-x86_64-apple-darwin.tar.gz")
sha=${sha%% *}

cd ../homebrew-kak-lsp/
sed 4c"  url \"$url\"" -i Formula/kak-lsp.rb
sed 5c"  sha256 \"$sha\"" -i Formula/kak-lsp.rb
sed 6c"  version \"$version\"" -i Formula/kak-lsp.rb
sed '4,6s/^/  /' -i Formula/kak-lsp.rb

git diff
read

git commit -am "$tag"
git push
