mod app;
mod bridge;
mod defaults;
mod menu;
mod navigation;
mod review;
mod state;
mod store;
mod watcher;
mod window;

use std::path::PathBuf;

fn main() {
    let mut print_html = false;
    let mut theme = mdcore::Theme::System;
    let mut want_theme = false;
    let mut paths: Vec<PathBuf> = Vec::new();

    for arg in std::env::args_os().skip(1) {
        let arg = arg.to_string_lossy().into_owned();

        if want_theme {
            want_theme = false;
            // A value that itself starts with "--" is another flag, not a
            // theme name: `--theme --print-html` must not swallow
            // `--print-html` as an (unrecognised, System-defaulting) theme
            // value. Treat the missing value as System — already the
            // default — and let this token fall through to be parsed
            // normally below instead of being consumed here.
            if !arg.starts_with("--") {
                theme = mdcore::Theme::from_wire(&arg);
                continue;
            }
        }

        match arg.as_str() {
            "--print-html" => print_html = true,
            "--theme" => want_theme = true,
            "--version" => {
                println!("mdview {}", mdcore::version());
                return;
            }
            "--help" | "-h" => {
                println!("usage: mdview [--print-html] [--theme THEME] [FILE...]");
                return;
            }
            other => paths.push(PathBuf::from(other)),
        }
    }

    if print_html {
        let Some(path) = paths.first() else {
            eprintln!("mdview: --print-html requires a file path");
            std::process::exit(2);
        };
        match mdcore::render_document(path, theme) {
            Ok(doc) => print!("{}", doc.html),
            Err(err) => {
                eprintln!("mdview: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    app::run(paths);
}

#[cfg(test)]
mod bundle_version_tests {
    /// The bundle carries its own version, independent of Cargo's. A release
    /// that ships them out of step reports one version in Finder's Get Info
    /// and another in `--version`, and there is nothing at runtime to catch
    /// it, so pin them here.
    #[test]
    fn the_bundle_version_matches_the_crate_version() {
        let plist = include_str!("../../../bundle/Info.plist");
        let key = "<key>CFBundleShortVersionString</key><string>";
        let start = plist.find(key).expect("no CFBundleShortVersionString") + key.len();
        let end = start + plist[start..].find('<').expect("unterminated version");
        assert_eq!(
            &plist[start..end],
            env!("CARGO_PKG_VERSION"),
            "bundle/Info.plist and Cargo.toml disagree about the version"
        );
    }

    /// The README documents the shortcut policy, so it has to move with it.
    /// The table is the SINGLE-KEY list now: the surviving modifier shortcuts
    /// were moved into the prose that introduces it rather than repeated in a
    /// second table, so they are asserted as mentions, not as rows.
    #[test]
    fn readme_documents_the_single_key_shortcuts_and_no_retired_ones() {
        let readme = include_str!("../../../README.md");
        for survivor in ["⌘O", "⌘F"] {
            assert!(readme.contains(survivor), "README should mention {survivor}");
        }
        for gone in ["⌥⌘F", "⌥⌘S", "⌥⌘D", "⌘G", "⇧⌘/", "| ⌘D |"] {
            assert!(!readme.contains(gone), "README still advertises {gone}");
        }
        assert!(readme.contains("| g w | Toggle fullwidth view |"));
    }

    /// Themes have no other native home now that the in-page picker is a
    /// palette the keyboard opens, so this wiring is the only way a mouse
    /// reaches them.
    #[test]
    fn the_theme_submenu_is_wired_from_the_menu_into_set_theme() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(selectTheme:)"));
        assert!(
            menu.contains("setRepresentedObject"),
            "each item must carry its own theme, not an index into a list"
        );
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(selectTheme:))]"));
        assert!(
            app.contains("Message::SetTheme"),
            "the menu must go through the same path the page does"
        );
        // Stamped at draw time, so a theme changed from the palette cannot
        // leave the menu showing a stale checkmark.
        assert!(app.contains("theme_menu_state"));
    }

    #[test]
    fn the_sidebar_tabs_are_reachable_without_the_keyboard() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(showOutline:)"));
        assert!(menu.contains("sel!(showBookmarks:)"));
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(showOutline:))]"));
        assert!(app.contains("#[unsafe(method(showBookmarks:))]"));
        assert!(app.contains("mdviewShowSidebarTab"));
    }

    #[test]
    fn the_comment_commands_are_reachable_without_the_keyboard() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(showComments:)"));
        assert!(menu.contains("sel!(copyReviewPrompt:)"));
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(showComments:))]"));
        assert!(app.contains("#[unsafe(method(copyReviewPrompt:))]"));
        // Through the same message the page's C sends, so the two cannot drift.
        assert!(app.contains("Message::CopyReview"));
    }

    /// Copying is news that expires, not a condition anyone has to resolve.
    /// It first shipped as a banner, which sits in the corner until clicked --
    /// so a shortcut you might press twice left two of them behind. Only the
    /// failed WRITE, which does need attention, stays a banner.
    #[test]
    fn copying_the_review_prompt_reports_through_the_transient_note() {
        let app = include_str!("app.rs");
        let start = app.find("fn copy_review_prompt(&self)").expect("no copy_review_prompt");
        let end = start + app[start..].find("\n    /// Send the bookmark").expect("end of fn");
        let body = &app[start..end];
        assert!(body.contains("show_note("), "C must speak through the note");
        assert!(!body.contains("show_banner("), "a banner would outlive the news");
        let window = include_str!("window.rs");
        assert!(window.contains("pub fn show_note"));
        // The error page has no init.js, so the call has to be guarded.
        assert!(window.contains("window.mdviewNote && window.mdviewNote("));
    }

    /// The same hazard as the first-run hint, and the one most likely to be
    /// got wrong twice: comments are pushed on every open and reload, which is
    /// exactly when loadHTMLString has returned but the page does not exist
    /// yet. Evaluating there runs the call against nothing and the anchors
    /// never appear, with no error anywhere.
    #[test]
    fn the_comment_list_is_queued_rather_than_evaluated() {
        let app = include_str!("app.rs");
        let start = app
            .find("pub(crate) fn push_comments_to_pages")
            .expect("the comment push must have a single funnel");
        let end = start + app[start..].find("\n    ///").expect("end of fn");
        let body = &app[start..end];
        assert!(body.contains("pending_scripts"));
        assert!(
            !body.contains("eval_script"),
            "drain_pending_banners owns the readiness check"
        );
        // One document's comments never reach another document's page.
        assert!(body.contains("window.path.borrow()"));
    }

    /// `C` asks Claude to delete the records it addressed, so something has
    /// to notice the file changing. Nothing else does: comments are re-read on
    /// open and after MDView's own writes, and an edit by anyone else would sit
    /// unseen until the document happened to be reloaded. The watch is its own
    /// per-window watcher, and it must be torn down with the document's --
    /// a stale one would report the PREVIOUS document's review into this window.
    #[test]
    fn the_review_file_is_watched_and_a_change_re_reads_it() {
        let window = include_str!("window.rs");
        assert!(window.contains("pub review_watcher: RefCell<Option<crate::watcher::FileWatcher>>"));
        assert!(window.contains("fn watch_review("), "the watch needs one funnel");
        // Started before the first comment exists, so the directory has to be
        // there: `save` does not make it until something is written.
        assert!(window.contains("review_watch_path"));
        let load = &window[window.find("pub fn load(").expect("load")..];
        let load = &load[..load.find("\n    pub fn ").expect("end of load")];
        assert!(load.contains("*self.review_watcher.borrow_mut() = None;"), "stale watch kept");
        assert!(load.contains("watch_review(path)"), "the new document is not watched");

        let app = include_str!("app.rs");
        let start = app.find("fn watch_tick").expect("no tick");
        let end = start + app[start..].find("\n        #[unsafe(method(").expect("end of tick");
        let tick = &app[start..end];
        assert!(tick.contains("review_watcher"), "nothing polls the review watch");
        assert!(
            tick.contains("push_comments_to_pages"),
            "a changed review has to be re-read, not just noticed"
        );
    }

    /// The gate that makes the review file safe to share with Claude.
    ///
    /// `serialize_review` renders the comment list and nothing else, so writing
    /// a file MDView only partly understood erases every record it had to skip.
    /// `C` asks Claude to delete records on every pass, which is exactly the
    /// edit that produces a half-deleted one — so the write has to refuse, and
    /// it has to say so, or comments stop being saved with nothing on screen
    /// to show it.
    #[test]
    fn a_review_that_cannot_be_wholly_read_is_never_written_over() {
        let app = include_str!("app.rs");
        // Through the single funnel every write goes through.
        let start = app.find("fn write_review").expect("no write_review");
        let end = start + app[start..].find("\n    /// Whether").expect("end of fn");
        let body = &app[start..end];
        assert!(body.contains("if self.review_is_damaged("), "the write is not gated");
        assert!(
            body.find("review_is_damaged").unwrap() < body.find("store::save").unwrap(),
            "the gate has to come before the write, not after it"
        );
        // Re-read at the gate rather than trusted from the caller, so an edit
        // made between their read and this write cannot slip through.
        let gate = app.find("fn review_is_damaged").expect("no gate");
        let gate_end = gate + app[gate..].find("\n    /// Send each page").expect("end of gate");
        assert!(app[gate..gate_end].contains("crate::store::load("));
        assert!(app[gate..gate_end].contains("damaged_review_banner"));

        // And the banner tracks the file rather than the moment: raised and
        // cleared by the funnel the review watcher runs.
        let push = app.find("pub(crate) fn push_comments_to_pages").expect("no push");
        let push_end = push + app[push..].find("\n    ///").expect("end of push");
        let push = &app[push..push_end];
        assert!(push.contains("damaged_review_banner"));
        assert!(push.contains("clear_banner"), "a fixed file must take its banner away");
        // Queued, not evaluated: this runs on every open, when the page it
        // would be injected into does not exist yet.
        assert!(push.contains("pending_banners"));
    }

    /// The reason `D` cannot open the diff has to reach the page, or the key
    /// falls back to a generic line and the three cases stop being told apart.
    /// Both hooks carry it: `mdviewSetViewState` runs on open, and
    /// `mdviewSetDiffAvailability` on every live reload, when a file can become
    /// tracked or a repository can gain its first commit.
    #[test]
    fn the_reason_the_diff_is_unavailable_reaches_the_page() {
        let window = include_str!("window.rs");
        assert!(window.contains("fn diff_reason_literal"), "no single funnel for the reason");
        assert!(window.contains("crate::state::diff_unavailable_note"));
        assert!(
            window.contains("diff_state: Cell<mdcore::DiffAvailability>"),
            "a bool cannot carry a reason"
        );
        assert_eq!(
            window.matches("self.diff_reason_literal()").count(),
            2,
            "both the open and the live-reload push must carry it"
        );
    }

    /// The one-time hint has to be QUEUED: loadHTMLString is asynchronous, so
    /// evaluating it directly would run against a page that does not exist yet.
    #[test]
    fn the_first_run_hint_is_queued_rather_than_evaluated() {
        let app = include_str!("app.rs");
        let start = app
            .find("fn maybe_queue_shortcuts_hint")
            .expect("the hint must have a single funnel");
        let end = start + app[start..].find("\n    /// Open the sidebar").expect("end of fn");
        let body = &app[start..end];
        assert!(body.contains("pending_scripts"));
        assert!(!body.contains("eval_script"), "drain_pending_banners owns the readiness check");
        assert!(body.contains("SHORTCUTS_HINT_SHOWN_KEY"));
    }

    #[test]
    fn readme_lists_the_cheat_sheet_shortcut() {
        let readme = include_str!("../../../README.md");
        assert!(readme.contains("| ? | The list of all of them |"));
        // The picker is gone; g t opens a palette.
        assert!(readme.contains("| g t | Themes |"));
        assert!(!readme.contains("Theme picker"));
    }

    /// The Help item and the page's own `?` have to reach the same sheet, or
    /// the menu would advertise something the page does not have.
    #[test]
    fn the_cheat_sheet_is_wired_from_the_help_menu_into_the_page() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(showShortcuts:)"));
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(showShortcuts:))]"));
        assert!(app.contains("crate::state::shortcuts_script()"));
    }

    #[test]
    fn fullwidth_native_action_is_wired_from_menu_through_reload() {
        let menu = include_str!("menu.rs");
        assert!(
            menu.contains("sel!(toggleFullWidth:)"),
            "View item must target the native action"
        );
        // Full Width has no key equivalent any more -- `w` in the page is the
        // only binding, so there is nothing here to keep in sync with it.
        // Scoped to the menu it builds: the tests below mention the setter.
        let install = &menu[menu.find("pub fn install(").expect("install")
            ..menu.find("#[cfg(test)]").expect("tests")];
        assert!(!install.contains("setKeyEquivalentModifierMask"));
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(toggleFullWidth:))]"));
        assert!(app.contains("set_bool(crate::defaults::FULL_WIDTH_KEY, enabled)"));
        assert!(app.contains("window.set_full_width(enabled)"));
        let window = include_str!("window.rs");
        assert!(window.contains("crate::defaults::FULL_WIDTH_KEY"));
        assert!(window.contains("crate::state::queue_full_width_script"));
        let start = window
            .find("pub fn set_full_width")
            .expect("window must queue native full-width changes");
        let end = start
            + window[start..]
                .find("pub(crate) fn eval_script")
                .expect("set_full_width must end before eval_script");
        let set_full_width = &window[start..end];
        assert!(set_full_width.contains("queue_full_width_script"));
        assert!(
            !set_full_width.contains("isLoading"),
            "drain_pending_banners owns the readiness check"
        );
        assert!(
            !set_full_width.contains("eval_script"),
            "full-width changes must stay queued until the page is ready"
        );
        assert!(window.contains("page_ready: Rc<Cell<bool>>"));
        assert_eq!(window.matches("self.page_ready.set(false)").count(), 2);
        assert!(window.contains("if !self.page_ready.get()"));
        let drain_start = window
            .find("pub fn drain_pending_banners")
            .expect("window must drain queued page state");
        let drain_end = drain_start
            + window[drain_start..]
                .find("pub fn set_full_width")
                .expect("drain_pending_banners must end before set_full_width");
        assert!(
            !window[drain_start..drain_end].contains("isLoading"),
            "navigation completion, not WebKit's handoff-time loading state, gates draining"
        );
        let navigation = include_str!("navigation.rs");
        assert!(navigation.contains("page_ready: Rc<Cell<bool>>"));
        assert!(navigation.contains("expected_navigation: Rc<RefCell<Option<Retained<WKNavigation>>>>"));
        assert!(navigation.contains("#[unsafe(method(webView:didFinishNavigation:))]"));
        assert!(navigation.contains("Retained::as_ptr(expected)"));
        assert!(navigation.contains("self.ivars().page_ready.set(true)"));
        assert!(window.contains("expected_navigation: Rc<RefCell<Option<Retained<WKNavigation>>>>"));
        assert!(window.contains("*self.expected_navigation.borrow_mut() = navigation"));
    }
}
