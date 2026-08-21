APP     := MDView.app
BINARY  := target/release/mdview
PREFIX  ?= /usr/local

.PHONY: all bundle install install-cli uninstall clean test

all: bundle

test:
	cargo test

$(BINARY):
	cargo build --release

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

clean:
	cargo clean
	rm -rf $(APP)
