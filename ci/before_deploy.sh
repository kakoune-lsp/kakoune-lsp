# This script takes care of building your crate and packaging it for release

set -ex

main() {
    local src=$(pwd) \
          stage=

    case $TRAVIS_OS_NAME in
        linux)
            stage=$(mktemp -d)
            ;;
        osx)
            stage=$(mktemp -d -t tmp)
            ;;
    esac

    test -f Cargo.lock || cargo generate-lockfile

    cross rustc --bin kak-lsp --target $TARGET --release -- -C lto

    cp target/$TARGET/release/kak-lsp $stage/
    cp kak-lsp.toml $stage/
    cp README.asciidoc $stage/

    case $TRAVIS_OS_NAME in
        linux)
            cp kak-lsp.service $stage/
            ;;
        osx)
            cp com.github.ul.kak-lsp.plist $stage/
            ;;
    esac

    cd $stage
    tar czf $src/$CRATE_NAME-$TRAVIS_TAG-$TARGET.tar.gz *
    cd $src

    rm -rf $stage
}

main
