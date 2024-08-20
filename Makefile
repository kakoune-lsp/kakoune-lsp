PKGNAME ?= kak-lsp
PREFIX ?= /usr

BIN_DIR = $(DESTDIR)$(PREFIX)/bin
SHARE_DIR = $(DESTDIR)$(PREFIX)/share

.PHONY: build install

build:
	cargo build --release --locked

install:
	install -Dm755 -t "$(BIN_DIR)" target/release/$(PKGNAME)
	install -Dm644 -t "$(SHARE_DIR)/$(PKGNAME)/rc/" rc/lsp.kak rc/servers.kak
	install -Dm644 UNLICENSE "$(SHARE_DIR)/licenses/$(PKGNAME)/LICENSE"

