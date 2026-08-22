APP     := MDView.app
BINARY  := target/release/mdview
PREFIX  ?= /usr/local
VERSION := $(shell awk -F'"' '/^version = /{print $$2; exit}' Cargo.toml)

# Which executable `bundle` packages. Local builds use the host-arch binary;
# `dist` overrides it with the universal one.
BUNDLE_BIN ?= $(BINARY)
ARCHS      := aarch64-apple-darwin x86_64-apple-darwin
UNIVERSAL  := target/universal/mdview

.PHONY: all bundle install install-cli uninstall clean test shot icon universal dist FORCE

all: bundle

test:
	cargo test

# FORCE gives $(BINARY) an always-out-of-date prerequisite, so `make bundle`
# always re-runs `cargo build --release` (cargo's own incremental build then
# decides what actually needs recompiling) instead of skipping straight to
# packaging whatever stale binary happens to already exist at $(BINARY).
$(BINARY): FORCE
	cargo build --release

FORCE:

bundle: $(BUNDLE_BIN) bundle/Info.plist bundle/MDView.icns
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Resources
	cp bundle/Info.plist $(APP)/Contents/Info.plist
	cp bundle/MDView.icns $(APP)/Contents/Resources/MDView.icns
	cp $(BUNDLE_BIN) $(APP)/Contents/MacOS/mdview
	# Carry the CLI shim inside the bundle so a dragged-in .app can still be
	# linked onto PATH without the repo.
	install -m 0755 scripts/mdview $(APP)/Contents/Resources/mdview
	# Ad-hoc: arm64 code will not run unsigned at all. It is not a Developer ID
	# signature, so downloaded copies still meet Gatekeeper -- see README.
	codesign --force --deep --sign - $(APP)
	@echo "built $(APP) $(VERSION)"

install: bundle
	rm -rf /Applications/$(APP)
	cp -R $(APP) /Applications/
	# Tell Launch Services about the document types immediately, instead of
	# waiting for it to notice the new bundle on its own schedule.
	/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
		-f /Applications/$(APP)
	@echo "installed /Applications/$(APP)"

install-cli:
	install -d $(PREFIX)/bin
	install -m 0755 scripts/mdview $(PREFIX)/bin/mdview
	@echo "installed $(PREFIX)/bin/mdview"

uninstall:
	rm -rf /Applications/$(APP)
	rm -f $(PREFIX)/bin/mdview

# Render a markdown file the way the app would and snapshot it to a PNG, so
# UI changes can be looked at instead of inferred from the stylesheet. The
# view is the same WebKit the app embeds; the AppKit bridge is not, so live
# reload, persistence and the native menu still need the running app.
#
#   make shot FILE=README.md
#   make shot FILE=notes.md THEME=mocha SIDEBAR=1 WIDTH=520 HEIGHT=420
#   make shot FILE=notes.md JS='document.getElementById("mdview-theme").open=true'
#
# SIDEBAR=1 opens the panel, which is hidden in a freshly generated page
# because the app normally opens it over the bridge. JS runs after load and is
# appended to that, for states the page does not start in.
FILE   ?=
THEME  ?=
WIDTH  ?= 900
HEIGHT ?= 700
SIDEBAR ?=
JS     ?=
SHOT_BIN := target/tools/shot
SHOT_OUT ?= target/shots/$(basename $(notdir $(FILE))).png

$(SHOT_BIN): tools/shot.swift
	@mkdir -p $(dir $@)
	swiftc -O -o $@ $<

shot: $(SHOT_BIN) $(BINARY)
ifeq ($(FILE),)
	$(error set FILE to a markdown file, e.g. make shot FILE=README.md)
endif
	@mkdir -p target/shots
	@$(BINARY) --print-html $(if $(THEME),--theme $(THEME),) $(FILE) > target/shots/page.html
	@$(SHOT_BIN) target/shots/page.html $(SHOT_OUT) $(WIDTH) $(HEIGHT) \
		'$(if $(SIDEBAR),document.getElementById("mdview-sidebar").hidden=false; document.querySelectorAll(".mdview-tab")[0].setAttribute("aria-selected","true");,) $(JS)'

# Redraw the app icon. bundle/MDView.icns is committed, so building the app
# needs no Swift toolchain; run this only after editing tools/icon.swift.
icon:
	@mkdir -p target/icon
	swiftc -O -o target/icon/icon tools/icon.swift
	rm -rf target/icon/MDView.iconset
	target/icon/icon target/icon/MDView.iconset
	iconutil -c icns target/icon/MDView.iconset -o bundle/MDView.icns
	@echo "wrote bundle/MDView.icns"

# A single binary carrying both architectures, so one download runs on Apple
# silicon and Intel alike.
universal:
	@for target in $(ARCHS); do \
		echo "cargo build --release --target $$target"; \
		cargo build --release --target $$target -p mdapp || exit 1; \
	done
	@mkdir -p target/universal
	lipo -create -output $(UNIVERSAL) $(foreach t,$(ARCHS),target/$(t)/release/mdview)
	@echo "universal: $$(lipo -archs $(UNIVERSAL))"

# A release DMG. Unsigned beyond ad-hoc, so first launch needs the Gatekeeper
# step in the README; there is no Developer ID to notarize with.
dist: test universal
	$(MAKE) bundle BUNDLE_BIN=$(UNIVERSAL)
	rm -rf dist target/dmg
	mkdir -p dist target/dmg
	cp -R $(APP) target/dmg/
	ln -s /Applications target/dmg/Applications
	hdiutil create -volname "MDView $(VERSION)" -srcfolder target/dmg \
		-ov -format UDZO dist/MDView-$(VERSION).dmg
	rm -rf target/dmg
	@echo
	@echo "dist/MDView-$(VERSION).dmg"
	@shasum -a 256 dist/MDView-$(VERSION).dmg

clean:
	cargo clean
	rm -rf $(APP) dist
