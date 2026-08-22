APP     := MDView.app
BINARY  := target/release/mdview
PREFIX  ?= /usr/local

.PHONY: all bundle install install-cli uninstall clean test shot FORCE

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

bundle: $(BINARY) bundle/Info.plist
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS
	cp bundle/Info.plist $(APP)/Contents/Info.plist
	cp $(BINARY) $(APP)/Contents/MacOS/mdview
	codesign --force --sign - $(APP)
	@echo "built $(APP)"

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

clean:
	cargo clean
	rm -rf $(APP)
