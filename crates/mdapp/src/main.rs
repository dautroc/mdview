mod app;
mod bridge;
mod croc;
mod defaults;
mod export;
mod menu;
mod navigation;
mod portable_review;
mod review;
mod review_index;
mod state;
mod store;
mod watcher;
mod window;

fn main() {
    let mut print_html = false;
    let mut open_only = false;
    let mut theme = mdcore::Theme::System;
    let mut want_theme = false;
    let mut diff = false;
    let mut interactive = false;
    let mut diff_layout = mdcore::DiffLayout::Unified;
    let mut want_diff_layout = false;
    let mut diff_base: Option<String> = None;
    let mut want_diff_base = false;
    let mut croc_code: Option<String> = None;
    let mut want_croc = false;
    let mut targets: Vec<state::OpenTarget> = Vec::new();

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

        if want_diff_base {
            want_diff_base = false;
            if !arg.starts_with("--") {
                diff_base = Some(arg);
                continue;
            }
        }

        // The same rule as --theme's and --diff-base's: the code is the next
        // argument unless it is another flag, in which case the missing value
        // is reported rather than a flag being swallowed as a code.
        if want_croc {
            want_croc = false;
            if !arg.starts_with("--") {
                croc_code = Some(arg);
                continue;
            }
        }

        // The same rule as --theme's: a value starting with "--" is the next
        // flag, not a layout name. An unrecognised name is worth refusing
        // rather than defaulting, because `--diff-layout renderd` silently
        // printing a unified diff is the kind of typo a demo bakes into a GIF.
        if want_diff_layout {
            want_diff_layout = false;
            if !arg.starts_with("--") {
                match mdcore::DiffLayout::from_wire(&arg) {
                    Some(layout) => {
                        diff_layout = layout;
                        continue;
                    }
                    None => {
                        eprintln!(
                            "mdview: unknown diff layout {arg:?} \
                             (unified, split, rendered, rendered-split)"
                        );
                        std::process::exit(2);
                    }
                }
            }
        }

        match arg.as_str() {
            "--print-html" => print_html = true,
            "--open" => open_only = true,
            "--interactive" => interactive = true,
            "--theme" => want_theme = true,
            "--diff" => diff = true,
            "--diff-layout" => want_diff_layout = true,
            "--diff-base" => want_diff_base = true,
            "--croc" => want_croc = true,
            "--version" => {
                println!("mdview {}", mdcore::version());
                return;
            }
            "--help" | "-h" => {
                println!(
                    "usage: mdview [--print-html [--interactive] [--diff \
                     [--diff-layout LAYOUT] [--diff-base REV]]] \
                     [--theme THEME] [--open] [--croc CODE] [FILE[:LINE]...]"
                );
                return;
            }
            // A path may carry the line to open at: `notes.md:42`, the way
            // everything that prints a location spells one.
            other => targets.push(state::parse_open_target(other)),
        }
    }

    // A code only ever means one thing: receive it now. The modes that print
    // or forward have no console to wait at and no inbox to fill, so mixing
    // them is refused rather than silently ignored.
    if want_croc {
        eprintln!("mdview: --croc requires a code");
        std::process::exit(2);
    }
    if croc_code.is_some()
        && (print_html || open_only || diff || interactive || diff_base.is_some())
    {
        eprintln!("mdview: --croc cannot be combined with other modes");
        std::process::exit(2);
    }
    // The receive decides which file opens -- the one the code names -- so a
    // path on the same line would be quietly ignored. Refuse it instead.
    if croc_code.is_some() && !targets.is_empty() {
        eprintln!("mdview: --croc opens the file the code names; it takes no path");
        std::process::exit(2);
    }

    if print_html {
        // A `:42` on the path is dropped rather than refused: printing a
        // document has no reading position to put anywhere, and the same
        // spelling has to work in both commands.
        let Some(path) = targets.first().map(|target| &target.path) else {
            eprintln!("mdview: --print-html requires a file path");
            std::process::exit(2);
        };
        // Which of the two pages to print. `--print-html` alone is File →
        // Export HTML from the command line, and that page deliberately drops
        // the reader's chrome, so it is the wrong thing to hand a tool that
        // exists to photograph the interface: `make shot` spent from v0.24.0
        // photographing a page with no sidebar and no key handling, and every
        // reel filmed one that ignored the keys it was pressing. `shot` and
        // `reel` ask for the other page; nothing else does.
        let purpose = if interactive {
            mdcore::RenderPurpose::Interactive
        } else {
            mdcore::RenderPurpose::Print
        };
        // The diff is a whole other page -- build_diff_page, not a state the
        // normal one can be put into -- so printing it needs its own branch
        // rather than a flag threaded through render_document. Without this
        // the four diff layouts are the one part of the app that cannot be
        // rendered outside it, which is why `make shot` has never shown one.
        let html = if diff {
            let highlighter = mdcore::Highlighter::new();
            let rendered = match diff_base {
                Some(base) => match mdcore::Revision::parse(base) {
                    Ok(base) => mdcore::render_diff_document_against_for_with(
                        path, &highlighter, theme, diff_layout, &base, purpose,
                    ),
                    Err(err) => {
                        eprintln!("mdview: {err}");
                        std::process::exit(2);
                    }
                },
                None => mdcore::render_diff_document_for_with(path, &highlighter, theme, diff_layout, purpose),
            };
            match rendered {
                Ok(doc) => doc.html,
                Err(err) => {
                    eprintln!("mdview: {err}");
                    std::process::exit(1);
                }
            }
        } else {
            let highlighter = mdcore::Highlighter::new();
            match mdcore::render_document_for_with(path, &highlighter, theme, purpose) {
                Ok(doc) => doc.html,
                Err(err) => {
                    eprintln!("mdview: {err}");
                    std::process::exit(1);
                }
            }
        };
        print!("{html}");
        return;
    }

    if open_only {
        // What the `mdview` shim runs. It cannot hand a `FILE:LINE` to `open`,
        // which stats the path, and it must not run the app itself: launching
        // the binary a second time gives a second copy of MDView instead of a
        // tab in the one already open. So the files become this app's own URLs
        // and go back through `open`, which is the same Apple event Finder
        // sends -- carrying the line as well as the path.
        if targets.is_empty() {
            eprintln!("mdview: --open requires a file path");
            std::process::exit(2);
        }
        let mut urls = Vec::with_capacity(targets.len());
        for target in &targets {
            // `open` says this, and says it before anything appears on screen.
            // Without it a mistyped name opens a tab reporting its own error,
            // which is the right answer for a file that went missing while it
            // was being read and the wrong one for a typo at the prompt.
            if !target.path.exists() {
                eprintln!("mdview: {} does not exist", target.path.display());
                std::process::exit(1);
            }
            // Absolute, because the URL is read by a process that was started
            // from somewhere else entirely.
            urls.push(state::open_url(&state::OpenTarget {
                path: watcher::watch_target(&target.path),
                line: target.line,
            }));
        }
        match std::process::Command::new("/usr/bin/open").args(&urls).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(err) => {
                eprintln!("mdview: could not run open: {err}");
                std::process::exit(1);
            }
        }
    }

    // Receive first, then hand the file to the app the same way every other
    // command-line open does: through the app's own URL, which is the Apple
    // event Finder sends. An MDView that is already open gains a tab instead
    // of a second copy of itself.
    if let Some(code) = croc_code {
        let Some(binary) = croc::find_croc() else {
            eprintln!("mdview: {}", croc::INSTALL_HINT);
            std::process::exit(1);
        };
        let Some(inbox) = store::croc_inbox_dir() else {
            eprintln!("mdview: {}", croc::message_for(&croc::CrocError::NoHome));
            std::process::exit(1);
        };
        let received = match croc::run_receive(&binary, &code, &inbox) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("mdview: {}", croc::message_for(&error));
                std::process::exit(1);
            }
        };
        let url = state::open_url(&state::OpenTarget {
            path: watcher::watch_target(&received),
            line: None,
        });
        match std::process::Command::new("/usr/bin/open").args([url]).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => {
                eprintln!("mdview: could not run open: {error}");
                std::process::exit(1);
            }
        }
    }

    if diff {
        eprintln!("mdview: --diff only applies to --print-html");
        std::process::exit(2);
    }

    if interactive {
        eprintln!("mdview: --interactive only applies to --print-html");
        std::process::exit(2);
    }
    if diff_base.is_some() {
        eprintln!("mdview: --diff-base requires --print-html --diff");
        std::process::exit(2);
    }

    app::run(targets);
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

    #[test]
    fn apple_event_files_arrive_before_workspace_restore() {
        let app = include_str!("app.rs");
        let launch_start = app.find("fn did_finish_launching").expect("launch callback");
        let launch_end = launch_start
            + app[launch_start..]
                .find("fn terminate_after_last_window")
                .expect("end of launch callback");
        assert!(
            !app[launch_start..launch_end].contains("restore_workspace_session"),
            "Finder and open -a files arrive after didFinishLaunching"
        );
        let untitled_start = app.find("fn open_untitled").expect("untitled callback");
        assert!(app[untitled_start..].contains("self.restore_workspace_session();"));
    }

    /// `mdview notes.md:42` has three joints that only meet at runtime: the
    /// argument is read as a path and a line, the tabs are opened, and the
    /// reveal is queued against the tab whose canonical path the opening
    /// resolved. A reveal queued against the spelling the reader typed finds
    /// no window and does nothing at all, silently.
    #[test]
    fn a_line_on_the_command_line_reaches_the_tab_it_opened() {
        let main = include_str!("main.rs");
        assert!(main.contains("state::parse_open_target(other)"));
        let app = include_str!("app.rs");
        let launch = app.find("fn did_finish_launching").expect("launch callback");
        let end = launch
            + app[launch..]
                .find("fn terminate_after_last_window")
                .expect("end of launch callback");
        assert!(
            app[launch..end].contains("self.reveal_target_lines(&targets);"),
            "the command line's line numbers are never acted on"
        );
        let reveal = app.find("fn reveal_target_lines").expect("the reveal");
        let body = &app[reveal..reveal + 900];
        assert!(
            body.contains("crate::watcher::watch_target(&target.path)"),
            "tabs are keyed by canonical path, so the lookup must be too"
        );
        assert!(
            body.contains("pending_scripts"),
            "the page is still loading when the tabs are made"
        );
    }

    /// The other half of that path, and the half that made the first one
    /// useless until it existed. The `mdview` command hands files to `open`,
    /// which stats the path and cannot carry anything beside it, so a
    /// `FILE:LINE` never reached the binary's argv at all. It goes through the
    /// app's own URL scheme instead: the shim asks the binary for the URLs,
    /// `open` routes them to the app already running, and the handler reads
    /// the line back out. Four files have to agree for any of that to happen.
    #[test]
    fn the_command_carries_a_line_to_an_already_running_app() {
        let shim = include_str!("../../../scripts/mdview");
        assert!(
            shim.contains(r#"*:[0-9]*) exec "$BIN" --open "$@" ;;"#),
            "the shim still sends FILE:LINE to `open`, which cannot take it"
        );
        let main = include_str!("main.rs");
        assert!(main.contains(r#""--open" => open_only = true,"#));
        assert!(
            main.contains(r#"Command::new("/usr/bin/open")"#),
            "--open must hand the URLs back to LaunchServices, not run the app"
        );
        let plist = include_str!("../../../bundle/Info.plist");
        let schemes = plist
            .find("<key>CFBundleURLSchemes</key>")
            .expect("the bundle declares no URL scheme");
        assert!(
            plist[schemes..schemes + 160]
                .contains(&format!("<string>{}</string>", crate::state::OPEN_SCHEME)),
            "the scheme the shim opens is not the one the bundle claims"
        );
        let app = include_str!("app.rs");
        let urls = app.find("fn open_urls").expect("the openURLs handler");
        let body = &app[urls..urls + 1600];
        assert!(
            body.contains("crate::state::parse_open_url"),
            "the handler drops every URL that is not a file, lines and all"
        );
        assert!(body.contains("self.reveal_target_lines(&targets);"));
    }

    /// `mdview --croc CODE` receives now, then opens what arrived the way every
    /// other command-line open does. Two halves have to agree: the flag reaches
    /// croc with its code, and the received file goes back through the app's
    /// own URL rather than a second launch of the app.
    #[test]
    fn the_command_line_can_receive_a_file_with_croc() {
        let main = include_str!("main.rs");
        assert!(main.contains(r#""--croc" => want_croc = true,"#));
        assert!(
            main.contains("croc::run_receive(&binary, &code, &inbox)"),
            "--croc must actually run a receive"
        );
        assert!(
            main.contains("state::open_url(&state::OpenTarget"),
            "the received file must open as a tab of the app already running"
        );
        // And the page binds the same command, so the key and the flag are one
        // feature rather than two.
        assert!(mdcore::assets::INIT_JS.contains(r#"postToHost("receiveCroc")"#));
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
        assert!(readme.contains("| g m | Toggle the minimap |"));
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
    fn every_opened_document_gets_a_native_tab_instead_of_repointing_one() {
        let app = include_str!("app.rs");
        assert!(app.contains("fn open_documents("));
        assert!(app.contains("addTabbedWindow_ordered"));
        assert!(!app.contains("open_first_record_rest"));
        assert!(!app.contains("existing.load("));
        assert!(
            app.contains("handle_message_from(Some(id), message)"),
            "a page callback must keep the tab that emitted it"
        );

        let window = include_str!("window.rs");
        assert!(
            !window.contains("pub fn load("),
            "a document tab must never be repointed at another path"
        );
    }

    #[test]
    fn tab_navigation_is_wired_through_both_native_and_vim_shortcuts() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(selectNextDocumentTab:)"));
        assert!(menu.contains("sel!(selectPreviousDocumentTab:)"));
        assert!(menu.contains("NSEventModifierFlags::Control"));

        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(selectNextDocumentTab:))]"));
        assert!(app.contains("#[unsafe(method(selectPreviousDocumentTab:))]"));
        assert!(app.contains("Message::NextTab"));
        assert!(app.contains("Message::PreviousTab"));
        assert!(
            app.contains("group.windows().len() > 1"),
            "tab navigation should be disabled when it cannot move"
        );

        let page = mdcore::assets::INIT_JS;
        assert!(page.contains("keys: [\"g n\"]"));
        assert!(page.contains("keys: [\"g p\"]"));
        assert!(
            page.contains("event.ctrlKey && !event.metaKey && !event.altKey && event.key === \"Tab\""),
            "WebKit must not consume Control-Tab as focus traversal"
        );
        assert!(page.contains("event.shiftKey ? \"previousTab\" : \"nextTab\""));
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
    fn intelligent_links_are_wired_through_native_and_page_surfaces() {
        let menu = include_str!("menu.rs");
        for selector in [
            "sel!(navigateBack:)",
            "sel!(navigateForward:)",
            "sel!(openCurrentLink:)",
            "sel!(openCurrentLinkInNewTab:)",
            "sel!(showLinks:)",
        ] {
            assert!(menu.contains(selector), "missing {selector}");
        }
        let app = include_str!("app.rs");
        assert!(app.contains("Message::PreviewLink"));
        assert!(app.contains("Message::OpenLink"));
        assert!(app.contains("replace_navigation_tab"));
        assert!(app.contains("LinkGraph::for_document"));
        let page = mdcore::assets::INIT_JS;
        assert!(page.contains("window.mdviewSetLinks"));
        assert!(page.contains("window.mdviewShowLinkPreview"));
        assert!(page.contains("mdview-broken-link"));
        assert!(page.contains("function previewDestination(anchor)"));
        assert!(page.contains("querySelectorAll(\"#mdview-content a[href]\")"));
        assert!(page.contains("keys: [\"g k\"]"));
        assert!(page.contains("keys: [\"g [\"]"));
        assert!(page.contains("keys: [\"g ]\"]"));
        assert!(page.contains("keys: [\"g i\"]"));
    }

    #[test]
    fn the_minimap_is_reachable_without_the_keyboard() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(toggleMinimap:)"));
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(toggleMinimap:))]"));
        // The page owns the strip; the menu item can only ask it to change.
        assert!(app.contains("crate::state::minimap_script(open)"));
    }

    #[test]
    fn print_and_pdf_export_are_reachable_from_the_file_menu() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(printDocument:)"));
        assert!(menu.contains("sel!(exportPdf:)"));
        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(printDocument:))]"));
        assert!(app.contains("#[unsafe(method(exportPdf:))]"));
        // Both share the export-only renderer and the readiness-gated capture.
        assert!(app.contains("crate::export::ReadyPage::load"));
        assert!(app.contains("RenderPurpose::Print"));
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
        let start = app
            .find("fn copy_review_prompt(&self, source_id: Option<u64>)")
            .expect("no copy_review_prompt");
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
        assert!(body.contains("window.path.to_string_lossy()"));
    }

    /// `C` asks Claude to delete the records it addressed, so something has
    /// to notice the file changing. Nothing else does: comments are re-read on
    /// open and after MDView's own writes, and an edit by anyone else would sit
    /// unseen until the document happened to be reloaded. Every tab owns one
    /// immutable document path and one review watcher, so closing that tab tears
    /// down exactly the resources that belong to it.
    #[test]
    fn the_review_file_is_watched_and_a_change_re_reads_it() {
        let window = include_str!("window.rs");
        assert!(window.contains("pub review_watcher: RefCell<Option<crate::watcher::FileWatcher>>"));
        assert!(window.contains("fn watch_review("), "the watch needs one funnel");
        // Started before the first comment exists, so the directory has to be
        // there: `save` does not make it until something is written.
        assert!(window.contains("review_watch_path"));
        assert!(
            window.contains("review_watcher: RefCell::new(watch_review(path))"),
            "a new tab does not start its review watch"
        );
        assert!(window.contains("pub path: PathBuf"), "tab identity must be immutable");
        assert!(
            !window.contains("pub fn load("),
            "a tab must not be repointed at another document"
        );

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

    /// The two rendered diff layouts, end to end. Each of these is inert on
    /// its own: a menu item with no action behind it does nothing, an action
    /// the validator never hears about stays enabled outside the diff, and a
    /// key the README and the page's own command list describe differently is
    /// two keys as far as a reader is concerned.
    #[test]
    fn the_rendered_diff_layouts_are_wired_from_the_menu_through_to_the_page() {
        let menu = include_str!("menu.rs");
        let app = include_str!("app.rs");
        let readme = include_str!("../../../README.md");
        for selector in ["setRenderedDiff:", "setRenderedSplitDiff:"] {
            assert!(menu.contains(&format!("sel!({selector})")), "no menu item for {selector}");
            assert!(
                app.contains(&format!("#[unsafe(method({selector}))]")),
                "no action for {selector}"
            );
            assert!(
                app.contains(&format!("objc2::sel!({selector})")),
                "{selector} is never validated, so it stays enabled outside the diff"
            );
        }
        let label = "Diff layout: source or rendered, one column or two";
        assert!(readme.contains(&format!("| g l | {label} |")), "the README row is stale");
        assert!(
            mdcore::assets::INIT_JS.contains(&format!("label: \"{label}\"")),
            "the cheat sheet and the palette say something else"
        );
    }

    #[test]
    fn document_history_is_wired_from_native_ui_through_each_tab_to_the_page() {
        let menu = include_str!("menu.rs");
        assert!(menu.contains("sel!(showDocumentHistory:)"));

        let app = include_str!("app.rs");
        assert!(app.contains("#[unsafe(method(showDocumentHistory:))]"));
        assert!(app.contains("Message::OpenHistory => self.request_history(source_id)"));
        assert!(app.contains("history_cache: RefCell<HashMap<u64, Vec<mdcore::HistoryEntry>>>"));
        assert!(app.contains("std::thread::spawn"), "Git history must not block AppKit");

        let window = include_str!("window.rs");
        assert!(window.contains("history_entry: RefCell<Option<mdcore::HistoryEntry>>"));

        let readme = include_str!("../../../README.md");
        assert!(readme.contains("| g h | Document history (needs a tracked file) |"));
        assert!(mdcore::assets::INIT_JS.contains("keys: [\"g h\"]"));
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

    /// The tour is the README's pictures, and a picture goes stale in a way
    /// prose does not: a GIF keeps showing the old interface long after the
    /// sentence beside it was corrected. These do not check that a demo is
    /// current -- nothing can -- but they do catch the two ways the tour rots
    /// on its own: a demo referenced but never generated, and a shortcut the
    /// app retired still being advertised in the one document that repeats
    /// them outside the table.
    #[test]
    fn every_demo_the_tour_shows_has_been_generated() {
        let tour = include_str!("../../../docs/tour.md");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut found = 0;
        for (at, _) in tour.match_indices("](gifs/") {
            let name = tour[at + 2..]
                .split(')')
                .next()
                .expect("an image reference must close");
            assert!(
                root.join(name).exists(),
                "docs/tour.md shows {name}, which does not exist -- run `make reels`"
            );
            found += 1;
        }
        assert_eq!(found, 12, "the tour is twelve sections, one demo each");
    }

    /// The same list main.rs already holds the README to. The tour is the only
    /// other document that names keys, so it is the only other one that can
    /// advertise a binding the app no longer has.
    #[test]
    fn the_tour_advertises_no_retired_shortcut() {
        let tour = include_str!("../../../docs/tour.md");
        for gone in ["⌥⌘F", "⌥⌘S", "⌥⌘D", "⌘G", "⇧⌘/", "Theme picker"] {
            assert!(!tour.contains(gone), "docs/tour.md still advertises {gone}");
        }
    }

    /// A spec that names a document nobody wrote fails at `make reels` with a
    /// WebKit load error, which is a long way from the file that caused it.
    #[test]
    fn every_reel_names_a_document_that_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let specs = std::fs::read_dir(root.join("reels")).expect("reels/ must exist");
        let mut seen = 0;
        for spec in specs {
            let path = spec.expect("a readable entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a readable spec");
            let page = text
                .split("\"page\":")
                .nth(1)
                .and_then(|rest| rest.split('"').nth(1))
                .unwrap_or_else(|| panic!("{path:?} names no starting page"));
            let doc = page.trim_end_matches(".html");
            // The diff reel starts from the throwaway repo's copy, which is
            // built by `make reel-pages` rather than committed.
            if doc == "proposal" {
                seen += 1;
                continue;
            }
            assert!(
                root.join("reels/demo").join(format!("{doc}.md")).exists(),
                "{path:?} starts from {doc}.md, which is not in reels/demo"
            );
            seen += 1;
        }
        assert_eq!(seen, 12, "twelve reels, one per section of the tour");
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

    /// Every page `shot` and `reel` generate has to be the interactive one.
    /// The two tools exist to show what the interface DOES, and the export
    /// page has no interface: from v0.24.0, when `--print-html` became File →
    /// Export HTML, `make shot SIDEBAR=1` opened a sidebar that was not in the
    /// page and every reel filmed eight copies of one frame. Nothing failed --
    /// the document still rendered -- so the assertion has to be here.
    #[test]
    fn every_page_the_demo_tools_generate_is_the_interactive_one() {
        let makefile = include_str!("../../../Makefile");
        let start = makefile.find("\nshot:").expect("the shot rule");
        let end = makefile[start..].find("\nreel:").expect("the reel rule") + start;
        let tools = &makefile[start..end];
        let generators: Vec<&str> = tools
            .lines()
            .filter(|line| line.contains("--print-html"))
            .collect();
        assert!(
            generators.len() >= 5,
            "expected shot and every reel page generator, saw {}",
            generators.len()
        );
        for line in generators {
            assert!(
                line.contains("--interactive"),
                "this page would be the export one, which has no interface to film: {}",
                line.trim()
            );
        }
    }

    /// A save that adds a document's FIRST diagram has to rebuild the page,
    /// not swap a body into it. `page.rs` inlines Mermaid only for a document
    /// that had a diagram when the page was built, so the swap would put the
    /// markup somewhere with nothing to draw it -- and nothing would say so:
    /// an undrawn diagram looks exactly like one that was meant to stay source.
    #[test]
    fn a_save_that_adds_the_first_diagram_rebuilds_the_page_rather_than_swapping() {
        let window = include_str!("window.rs");
        // The flag is recorded from the render that built the page, beside the
        // load it describes -- not recomputed later from a body that may have
        // moved on.
        assert!(window.contains("self.page_has_diagrams.set(doc.has_diagrams)"));
        let start = window
            .find("pub fn live_update(")
            .expect("the live reload path");
        let body = &window[start..];
        let branch = body
            .find("mdcore::page::needs_diagrams(&body)")
            .expect("the swap must ask whether the new body needs a runtime");
        let swap = body
            .find("target.innerHTML = {body};")
            .expect("the swap itself");
        assert!(
            branch < swap,
            "the question has to be asked before the body is swapped in"
        );
        assert!(
            body[branch..swap].contains("self.reload(highlighter)"),
            "the answer is a full rebuild, which is the only thing that inlines the runtime"
        );
    }

    #[test]
    fn fullwidth_native_action_is_wired_from_menu_through_reload() {
        let menu = include_str!("menu.rs");
        assert!(
            menu.contains("sel!(toggleFullWidth:)"),
            "View item must target the native action"
        );
        // Full Width itself still has no key equivalent; the only explicit
        // modifier masks in this menu belong to native tab navigation.
        assert!(menu.contains(
            "item(mtm, FULL_WIDTH_TITLE, sel!(toggleFullWidth:), \"\")"
        ));
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
