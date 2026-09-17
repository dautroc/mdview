// The Mermaid runtime, shared by both page runtimes.
//
// `init.js` and `export.js` each used to carry their own copy of this, byte
// for byte, with nothing holding the two in sync -- so a change to the
// configuration was a change anyone could make in one of them and ship. It
// lives here once instead, and both call it through a shim that degrades to a
// resolved promise, which is also what a page built without a diagram gets:
// `page.rs` inlines this file, the Mermaid bundle and the ELK bundle only when
// the document actually draws something.
(function () {
  "use strict";

  // Stash each diagram's ORIGINAL source before mermaid replaces it with an
  // SVG. A previous attempt re-read the rendered output as if it were source,
  // which fed mermaid its own SVG and corrupted the diagram. Source is only
  // recoverable before the first render, so capture it here.
  function stashMermaidSources() {
    var nodes = document.querySelectorAll("pre.mermaid");
    for (var i = 0; i < nodes.length; i++) {
      if (!nodes[i].hasAttribute("data-mermaid-src")) {
        nodes[i].setAttribute("data-mermaid-src", nodes[i].textContent);
      }
    }
  }

  // A named theme's wire value ("mocha", "github", ...) does not say
  // whether it is dark, so JS cannot derive it -- only Rust can, from
  // Theme::is_dark. Rust stamps that darkness onto the html element as a
  // data-dark attribute (1 for dark, 0 for light), alongside data-theme.
  // Only System has no stamp, and defers to the OS media query -- reading
  // the query for a named theme would render every diagram in the OS
  // palette while the rest of the page honours the user's choice.
  function effectiveTheme() {
    var stamped = document.documentElement.getAttribute("data-dark");
    if (stamped === "1") return "dark";
    if (stamped === "0") return "light";
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  // The theme's own tokens, RESOLVED -- not passed through as `var(--bg)`.
  // Mermaid's `base` theme runs these through khroma to derive the shades it
  // was not given (a lighter node fill, a darker border), and khroma parses
  // colours, not CSS functions: a var() reaches it as a string it cannot read.
  // chrome.rs stamps every one of these per named theme and page.css defines
  // them all at :root for System, so the fallbacks here are only ever reached
  // on a page that has no stylesheet at all.
  function diagramPalette(dark) {
    var styles = getComputedStyle(document.documentElement);
    function token(name, fallback) {
      var value = styles.getPropertyValue(name).trim();
      return value || fallback;
    }

    var bg = token("--bg", dark ? "#0d1117" : "#ffffff");
    var fg = token("--fg", dark ? "#e6edf3" : "#1f2328");
    var muted = token("--muted", dark ? "#9198a1" : "#59636e");
    var border = token("--border", dark ? "#3d444d" : "#d1d9e0");
    var surface = token("--code-bg", dark ? "#161b22" : "#f6f8fa");
    var link = token("--link", dark ? "#4493f8" : "#0969da");

    return {
      darkMode: dark,
      background: bg,
      // A node is the page's code background on the page's background: the
      // same pairing a fenced block already uses, so a diagram reads as part
      // of the document rather than as a picture dropped into it.
      mainBkg: surface,
      primaryColor: surface,
      secondaryColor: surface,
      tertiaryColor: bg,
      primaryTextColor: fg,
      secondaryTextColor: fg,
      tertiaryTextColor: fg,
      nodeTextColor: fg,
      textColor: fg,
      titleColor: fg,
      primaryBorderColor: border,
      secondaryBorderColor: border,
      tertiaryBorderColor: border,
      nodeBorder: border,
      clusterBkg: bg,
      clusterBorder: border,
      // Edges are quieter than the text they connect, the way a table's rules
      // are quieter than its cells.
      lineColor: muted,
      // An edge label sits ON an edge, so it needs the page's background
      // behind it or the line draws straight through the words.
      edgeLabelBackground: bg,
      labelBackground: bg,
      labelBoxBkgColor: surface,
      labelBoxBorderColor: border,
      labelTextColor: fg,
      noteBkgColor: surface,
      noteTextColor: fg,
      noteBorderColor: link,
      actorBkg: surface,
      actorBorder: border,
      actorTextColor: fg,
      signalColor: muted,
      signalTextColor: fg,
    };
  }

  // Read rather than hardcoded, so page.css's body rule stays the one place
  // the document's typeface is written down. Mermaid sizes flowchart labels
  // from the real DOM (they are HTML in a foreignObject), so the font it
  // measures and the font it draws are necessarily the same one.
  function bodyFontFamily() {
    var body = document.body;
    var family = body ? getComputedStyle(body).fontFamily : "";
    return family || '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, sans-serif';
  }

  // Registered once. renderDiagrams runs again on every live reload, and
  // registering the same loaders on each pass would stack them up.
  var elkRegistered = false;
  function elkAvailable() {
    if (elkRegistered) return true;
    var layouts = window.mdviewElkLayouts;
    if (!layouts || typeof mermaid.registerLayoutLoaders !== "function") return false;
    mermaid.registerLayoutLoaders(layouts);
    elkRegistered = true;
    return true;
  }

  function diagramConfig() {
    var dark = effectiveTheme() === "dark";
    return {
      startOnLoad: false,
      // Load-bearing against the page's CSP, which is `default-src 'none'`
      // with a nonce-only script-src: the looser levels want an iframe.
      securityLevel: "strict",
      // `base` is the only theme that treats themeVariables as authoritative;
      // `default` and `dark` ignore most of what is set below.
      theme: "base",
      themeVariables: diagramPalette(dark),
      fontFamily: bodyFontFamily(),
      // Named only when ELK is actually registered. Mermaid throws on a layout
      // it has no loader for, and that throw takes the diagram down rather
      // than falling back -- so a page built without the bundle says dagre.
      layout: elkAvailable() ? "elk" : "dagre",
      elk: {
        // Edges that share a path are prettier and harder to follow, which is
        // the whole complaint this configuration exists to answer.
        mergeEdges: false,
        nodePlacementStrategy: "BRANDES_KOEPF",
        // Which edge of a cycle gets reversed decides what ELK thinks the
        // first layer is, and the default degree-based answer is wrong for the
        // shape documents actually draw: a queue that feeds back on itself
        // ranked `Slot free?` at the top and left `Upload`, the entry, halfway
        // down. A depth-first walk from the real sources classifies back-edges
        // the way a reader following the arrows would, and puts the entry
        // first. `keepEntryNodeOnTop` covers the case DFS cannot -- a loop with
        // no way in, where every node has an incoming edge and there is no
        // source to walk from.
        cycleBreakingStrategy: "DEPTH_FIRST",
        keepEntryNodeOnTop: true,
      },
      flowchart: {
        // Orthogonal routes with rounded corners, against mermaid's default
        // `basis` -- a spline through the route's control points, which bulges
        // away from the path and into whatever is beside it.
        curve: "rounded",
        // Mermaid's default is 8. GitHub uses 48, and the room is most of why
        // their diagrams read as composed rather than crowded.
        diagramPadding: 48,
        nodeSpacing: 60,
        rankSpacing: 70,
        wrappingWidth: 220,
        // openLightbox in init.js strips the width/height and inline max-width
        // this writes. Turning it off would leave the zoom fighting an
        // explicit size.
        useMaxWidth: true,
      },
      sequence: { diagramMarginY: 40 },
    };
  }

  // Always resolves (never rejects), and resolves synchronously-ish via a
  // microtask even when mermaid is absent or throws, so callers can chain
  // off it unconditionally without a try/catch of their own.
  function renderDiagrams() {
    if (typeof mermaid === "undefined") return Promise.resolve();
    stashMermaidSources();
    try {
      mermaid.initialize(diagramConfig());
      var result = mermaid.run({ querySelector: "pre.mermaid" });
      if (result && typeof result.then === "function") {
        return result.catch(function () {
          /* leave the diagram source visible as text */
        });
      }
      return Promise.resolve();
    } catch (err) {
      /* leave the diagram source visible as text */
      return Promise.resolve();
    }
  }

  window.mdviewRenderDiagrams = renderDiagrams;
})();
