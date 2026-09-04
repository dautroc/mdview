APP     := MDView.app
BINARY  := target/release/mdview
PREFIX  ?= /usr/local
# The version as it stands. VERSION is reserved for `make version`'s argument.
CURRENT := $(shell awk -F'"' '/^version = /{print $$2; exit}' Cargo.toml)

# Which executable `bundle` packages. Local builds use the host-arch binary;
# `dist` overrides it with the universal one.
BUNDLE_BIN ?= $(BINARY)
ARCHS      := aarch64-apple-darwin x86_64-apple-darwin
UNIVERSAL  := target/universal/mdview

.PHONY: all bundle install install-cli uninstall clean test shot reel reels reel-pages \
	reel-size icon universal dist version FORCE

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
	@echo "built $(APP) $(CURRENT)"

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
# because the app normally opens it over the bridge. It goes through the page's
# own mdviewShowSidebarTab -- the hook the View menu uses -- rather than poking
# the markup, so a shot cannot drift from the app the way it did when this
# reached for the sidebar tabs that v0.9.0 deleted. SIDEBAR=bookmarks picks the
# other panel. JS runs after load and is appended to that, for states the page
# does not start in.
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
		'$(if $(SIDEBAR),window.mdviewShowSidebarTab("$(if $(filter bookmarks,$(SIDEBAR)),bookmarks,outline)");,) $(JS)'

# Record a sequence of page states and assemble them into a GIF, so docs/tour.md
# can show what a key DOES rather than describing it. `shot` answers "does this
# look right"; a reel answers "what happens when you press this", which is the
# question the README has been failing to answer in prose.
#
#   make reels                 # every spec in reels/
#   make reel REEL=finding     # one of them
#   make reel-pages            # just the pages the specs load
#   make reel-size             # what the committed gifs weigh
#
# A reel drives the same generated page `shot` snapshots, through the same
# window.mdview* hooks the AppKit bridge calls -- and reel.swift registers the
# bridge's own "mdview" message handler, so c, C, g d, g r and m behave as they
# do in the app rather than reporting that they need it.
#
# ffmpeg is a DEVELOPER's dependency and nothing else: it is not in the binary,
# not in the bundle, and `all`, `test` and `dist` never reach any of this.
# Without it the frames are still written, and the target says where they are.
REELS_DIR  ?= reels
GIF_DIR    ?= docs/gifs
GIF_WIDTH  ?= 900
GIF_COLORS ?= 128
REEL_THEME ?= solarized-light
REEL       ?=
FFMPEG     ?= ffmpeg
REEL_BIN   := target/tools/reel
REEL_WORK  := target/reels
REEL_NAMES := $(basename $(notdir $(wildcard $(REELS_DIR)/*.json)))

$(REEL_BIN): tools/reel.swift
	@mkdir -p $(dir $@)
	swiftc -O -o $@ $<

# Every page any spec can load. Images are inlined as data: URIs, so a
# generated page is self-contained and can live under target/ whatever its
# markdown does.
#
# The diff pages need a repository with a commit to diff against, and the demo
# document is not committed in a state that has one -- so a throwaway repo is
# built under target/. The working tree is never dirtied to film it, and no
# repository of the reader's is ever read.
reel-pages: $(BINARY)
	@mkdir -p $(REEL_WORK)/pages
	@for doc in $(REELS_DIR)/demo/*.md; do \
		name=$$(basename $$doc .md); \
		case $$name in changes.*) continue;; esac; \
		$(BINARY) --print-html --theme $(REEL_THEME) $$doc \
			> $(REEL_WORK)/pages/$$name.html; \
	done
	@# The theme reel commits a theme, and the app answers that by rebuilding
	@# the page -- so every theme it visits needs its own page, or the frame
	@# would keep the syntax colours of the theme it just left.
	@for theme in github solarized-dark mocha chiroptera-dark-hard; do \
		$(BINARY) --print-html --theme $$theme $(REELS_DIR)/demo/tour.md \
			> $(REEL_WORK)/pages/tour.$$theme.html; \
	done
	@rm -rf $(REEL_WORK)/git && mkdir -p $(REEL_WORK)/git
	@cp $(REELS_DIR)/demo/changes.before.md $(REEL_WORK)/git/proposal.md
	@cd $(REEL_WORK)/git && git init -q . \
		&& git -c user.email=reel@localhost -c user.name=reel add proposal.md \
		&& git -c user.email=reel@localhost -c user.name=reel commit -qm before
	@cp $(REELS_DIR)/demo/changes.after.md $(REEL_WORK)/git/proposal.md
	@for layout in unified split rendered rendered-split; do \
		$(BINARY) --print-html --diff --diff-layout $$layout --theme $(REEL_THEME) \
			$(REEL_WORK)/git/proposal.md \
			> $(REEL_WORK)/pages/proposal.$$layout.html; \
	done
	@$(BINARY) --print-html --theme $(REEL_THEME) $(REEL_WORK)/git/proposal.md \
		> $(REEL_WORK)/pages/proposal.html
	@echo "pages in $(REEL_WORK)/pages"

reel: $(REEL_BIN) reel-pages
ifeq ($(REEL),)
	$(error set REEL to a spec in $(REELS_DIR), e.g. make reel REEL=finding)
endif
	@test -f $(REELS_DIR)/$(REEL).json \
		|| { echo "no such reel: $(REELS_DIR)/$(REEL).json" >&2; exit 2; }
	@rm -rf $(REEL_WORK)/$(REEL) && mkdir -p $(REEL_WORK)/$(REEL) $(GIF_DIR)
	@$(REEL_BIN) $(REEL_WORK)/pages/$$(awk -F'"' '/"page":/{print $$4; exit}' \
		$(REELS_DIR)/$(REEL).json) $(REELS_DIR)/$(REEL).json $(REEL_WORK)/$(REEL)
	@# One recipe line, because the frames are the expensive half and are
	@# worth keeping: without ffmpeg this says where they are and stops,
	@# rather than failing on a command the app itself never needs.
	@if ! command -v $(FFMPEG) >/dev/null 2>&1; then \
		echo "$(FFMPEG) not found: the frames are in $(REEL_WORK)/$(REEL)."; \
		echo "  brew install ffmpeg   to assemble them"; \
		exit 0; \
	fi; \
	set -e; \
	$(FFMPEG) -y -loglevel error -f concat -safe 0 \
		-i $(REEL_WORK)/$(REEL)/frames.txt \
		-vf "scale=$(GIF_WIDTH):-2:flags=lanczos,palettegen=max_colors=$(GIF_COLORS):stats_mode=diff" \
		-update 1 $(REEL_WORK)/$(REEL)/palette.png; \
	$(FFMPEG) -y -loglevel error -f concat -safe 0 \
		-i $(REEL_WORK)/$(REEL)/frames.txt -i $(REEL_WORK)/$(REEL)/palette.png \
		-filter_complex "[0:v]scale=$(GIF_WIDTH):-2:flags=lanczos[s];[s][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
		-fps_mode vfr -loop 0 $(GIF_DIR)/$(REEL).gif; \
	echo "$(GIF_DIR)/$(REEL).gif  $$(du -h $(GIF_DIR)/$(REEL).gif | cut -f1)"

reels: $(REEL_BIN) reel-pages
	@for name in $(REEL_NAMES); do \
		$(MAKE) --no-print-directory reel REEL=$$name || exit 1; done
	@$(MAKE) --no-print-directory reel-size

# Committed weight, said out loud. Seven reels should come to about five
# megabytes; past that, cut frames before cutting quality.
reel-size:
	@du -ch $(GIF_DIR)/*.gif 2>/dev/null | tail -1

# Redraw the app icon. bundle/MDView.icns is committed, so building the app
# needs no Swift toolchain; run this only after editing tools/icon.swift.
icon:
	@mkdir -p target/icon
	swiftc -O -o target/icon/icon tools/icon.swift
	rm -rf target/icon/MDView.iconset
	target/icon/icon target/icon/MDView.iconset
	iconutil -c icns target/icon/MDView.iconset -o bundle/MDView.icns
	@echo "wrote bundle/MDView.icns"

# Bump the version everywhere it is recorded, then prove the copies agree.
#
#   make version VERSION=0.2.0
#
# The version lives in Cargo.toml, and the bundle keeps its own copy that
# nothing at runtime would reconcile. CFBundleVersion is a build counter Apple
# expects to rise on every shipped build, so it advances too. Committing and
# pushing the result is enough to cut a release; CI notices the new version.
version:
	@test -n "$(VERSION)" || { echo "usage: make version VERSION=x.y.z" >&2; exit 1; }
	@echo "$(VERSION)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$$' \
		|| { echo "version must look like x.y.z, got '$(VERSION)'" >&2; exit 1; }
	@test "$(VERSION)" != "$(CURRENT)" || { echo "already at $(CURRENT)" >&2; exit 1; }
	@git rev-parse -q --verify "refs/tags/v$(VERSION)" >/dev/null \
		&& { echo "v$(VERSION) is already tagged" >&2; exit 1; } || true
	@sed -i '' '1,/^version = /s/^version = ".*"/version = "$(VERSION)"/' Cargo.toml
	@sed -i '' 's|<key>CFBundleShortVersionString</key><string>.*</string>|<key>CFBundleShortVersionString</key><string>$(VERSION)</string>|' bundle/Info.plist
	@build=$$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' bundle/Info.plist); \
		next=$$((build + 1)); \
		sed -i '' "s|<key>CFBundleVersion</key><string>.*</string>|<key>CFBundleVersion</key><string>$$next</string>|" bundle/Info.plist; \
		echo "  build $$build -> $$next"
	@# Building refreshes Cargo.lock, which pins the workspace members' versions,
	@# and runs the test that fails when the two copies disagree.
	@cargo test -q -p mdapp bundle_version >/dev/null
	@echo "  Cargo.toml:  $$(awk -F'"' '/^version = /{print $$2; exit}' Cargo.toml)"
	@echo "  Info.plist:  $$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' bundle/Info.plist)"
	@echo "  Cargo.lock:  $$(grep -A1 '^name = "mdapp"' Cargo.lock | awk -F'"' '/^version/{print $$2}')"
	@echo
	@echo "next: git commit -am 'release $(VERSION)' && git push"

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
	hdiutil create -volname "MDView $(CURRENT)" -srcfolder target/dmg \
		-ov -format UDZO dist/MDView-$(CURRENT).dmg
	rm -rf target/dmg
	@echo
	@echo "dist/MDView-$(CURRENT).dmg"
	@shasum -a 256 dist/MDView-$(CURRENT).dmg

clean:
	cargo clean
	rm -rf $(APP) dist
