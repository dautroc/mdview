(function () {
  function renderMath() {
    if (typeof katex === "undefined") return;
    var nodes = document.querySelectorAll(".math-inline, .math-display");
    for (var i = 0; i < nodes.length; i++) {
      var node = nodes[i];
      var tex = node.textContent;
      try {
        katex.render(tex, node, {
          displayMode: node.classList.contains("math-display"),
          throwOnError: false,
        });
      } catch (err) {
        // A malformed expression shows as literal TeX rather than blanking
        // the surrounding paragraph.
        node.textContent = tex;
      }
    }
  }

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

  // The explicit theme lives in <html data-theme>, which does NOT affect
  // prefers-color-scheme. Reading the media query here would render every
  // diagram in the OS palette while the rest of the page honours the user's
  // choice. Fall back to the query only when no explicit theme is pinned.
  function effectiveTheme() {
    var pinned = document.documentElement.getAttribute("data-theme");
    if (pinned === "dark" || pinned === "light") return pinned;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  // Always resolves (never rejects), and resolves synchronously-ish via a
  // microtask even when mermaid is absent or throws, so callers can chain
  // off it unconditionally without a try/catch of their own.
  function renderDiagrams() {
    if (typeof mermaid === "undefined") return Promise.resolve();
    stashMermaidSources();
    try {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: effectiveTheme() === "dark" ? "dark" : "default",
      });
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

  // ---- Click-to-zoom ---------------------------------------------------
  //
  // One overlay (#mdview-lightbox), created lazily and appended to
  // document.body -- deliberately OUTSIDE #mdview-content, because live
  // reload replaces that div's innerHTML and an overlay living inside it
  // would be destroyed mid-use.

  var MIN_SCALE = 0.25;
  var MAX_SCALE = 8;

  var zoomState = null; // { scale, x, y } while the overlay is open, else null
  var dragState = null;
  var savedScrollY = 0;
  var savedBodyOverflow = "";

  function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
  }

  function wrapZoomable(node, inline) {
    if (!node || node.hasAttribute("data-mdview-zoom")) return;
    node.setAttribute("data-mdview-zoom", "1");

    var wrapper = document.createElement("span");
    wrapper.className = inline
      ? "mdview-zoomable mdview-zoomable-inline"
      : "mdview-zoomable";
    node.parentNode.insertBefore(wrapper, node);
    wrapper.appendChild(node);

    // Clicking the diagram/image itself opens the overlay -- except when the
    // author wrapped it in a link (e.g. [![alt](img)](https://...)), in
    // which case the click should follow the link instead. The zoom button
    // (below) still works in that case via its own handler.
    wrapper.addEventListener("click", function () {
      var content = document.getElementById("mdview-content");
      var link = wrapper.closest("a");
      if (link && content && content.contains(link)) return;
      openLightbox(node);
    });

    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "mdview-zoom-btn";
    btn.setAttribute("aria-label", "Zoom");
    btn.textContent = "⤢"; // NE arrow and SW arrow: a compact "expand" glyph
    btn.addEventListener("click", function (event) {
      // preventDefault() stops a wrapping <a> from navigating when this
      // click bubbles to it; stopPropagation() stops it reaching the
      // wrapper's own click handler above, which would otherwise open the
      // overlay a second time (or reopen it immediately after this closes).
      event.preventDefault();
      event.stopPropagation();
      openLightbox(node);
    });
    wrapper.appendChild(btn);
  }

  // Walks #mdview-content for Mermaid diagrams and images and wraps each in
  // a zoomable affordance. Safe to call repeatedly: every processed node
  // carries data-mdview-zoom, so a second pass (e.g. after a live-reload
  // save re-runs mdviewRenderAll) is a no-op for anything already wrapped.
  function enhanceZoomables() {
    var content = document.getElementById("mdview-content");
    if (!content) return;

    var diagrams = content.querySelectorAll("pre.mermaid");
    for (var i = 0; i < diagrams.length; i++) {
      var pre = diagrams[i];
      var svg = pre.querySelector("svg");
      wrapZoomable(svg || pre, false);
    }

    var images = content.querySelectorAll("img");
    for (var j = 0; j < images.length; j++) {
      wrapZoomable(images[j], true);
    }
  }

  // Chrome colours come from CSS custom properties keyed off data-theme, but
  // the syntect highlight palettes are whole stylesheets selected by a media
  // attribute (they cannot be nested under a selector — that needs CSS
  // Nesting, unsupported before macOS 13.4). Both must move together or the
  // page recolours while code blocks stay behind.
  function applyHighlightSheets(theme) {
    var light = document.getElementById("mdview-hl-light");
    var dark = document.getElementById("mdview-hl-dark");
    if (!light || !dark) return;
    if (theme === "light") {
      light.media = "all";
      dark.media = "not all";
    } else if (theme === "dark") {
      light.media = "not all";
      dark.media = "all";
    } else {
      light.media = "all";
      dark.media = "(prefers-color-scheme: dark)";
    }
  }

  // Restore every diagram to its stashed source and let mermaid render it
  // again under the new theme. mermaid skips nodes carrying data-processed,
  // so that attribute must go too.
  function rerenderDiagrams() {
    var nodes = document.querySelectorAll("pre.mermaid[data-mermaid-src]");
    // Nothing to re-theme. Skip rather than paying mermaid.initialize() and
    // run() on every theme toggle for a document that has no diagrams.
    if (!nodes.length) return Promise.resolve();
    for (var i = 0; i < nodes.length; i++) {
      var node = nodes[i];
      node.removeAttribute("data-processed");
      node.textContent = node.getAttribute("data-mermaid-src");
    }
    return renderDiagrams();
  }

  // Called from Rust when the theme changes. KaTeX needs no re-render: its
  // output inherits colour from CSS. Only mermaid bakes the theme in.
  var themeFrame = 0;
  var themeGeneration = 0;

  window.mdviewApplyTheme = function (theme) {
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else {
      document.documentElement.setAttribute("data-theme", theme);
    }
    applyHighlightSheets(theme);
    // Everything above is an attribute flip and repaints instantly — but the
    // browser cannot paint until this task yields, and re-rendering diagrams
    // does substantial synchronous work in mermaid. Defer it a frame so the
    // theme change is visible immediately and the diagrams catch up.
    //
    // rerenderDiagrams() destroys and rebuilds every pre.mermaid node, taking
    // the zoom wrapper and button with it (wrapZoomable inserts them inside
    // that node). Chain the enhancer exactly as mdviewRenderAll does, or
    // diagrams lose click-to-zoom until the next save.
    // Coalesce rapid changes. Each re-render lays out every diagram in
    // mermaid, so N quick clicks would otherwise queue N full re-layouts that
    // serialise and lag behind the clicks. Cancel any frame not yet run, and
    // tag each attempt so a superseded render cannot re-enhance after a newer
    // one has already finished.
    if (themeFrame) cancelAnimationFrame(themeFrame);
    var generation = ++themeGeneration;
    themeFrame = requestAnimationFrame(function () {
      themeFrame = 0;
      rerenderDiagrams().then(function () {
        if (generation === themeGeneration) enhanceZoomables();
      });
    });
  };

  function getLightbox() {
    var existing = document.getElementById("mdview-lightbox");
    if (existing) return existing;

    var overlay = document.createElement("div");
    overlay.id = "mdview-lightbox";
    overlay.hidden = true;

    var stage = document.createElement("div");
    stage.className = "mdview-lightbox-stage";

    var inner = document.createElement("div");
    inner.className = "mdview-lightbox-inner";
    stage.appendChild(inner);

    var closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "mdview-lightbox-close";
    closeBtn.setAttribute("aria-label", "Close");
    closeBtn.textContent = "×"; // ×

    var controls = document.createElement("div");
    controls.className = "mdview-lightbox-controls";

    var zoomOutBtn = document.createElement("button");
    zoomOutBtn.type = "button";
    zoomOutBtn.className = "mdview-lightbox-btn";
    zoomOutBtn.setAttribute("aria-label", "Zoom out");
    zoomOutBtn.textContent = "−"; // −

    var resetBtn = document.createElement("button");
    resetBtn.type = "button";
    resetBtn.className = "mdview-lightbox-btn";
    resetBtn.setAttribute("aria-label", "Reset zoom");
    resetBtn.textContent = "↺"; // ↺

    var zoomInBtn = document.createElement("button");
    zoomInBtn.type = "button";
    zoomInBtn.className = "mdview-lightbox-btn";
    zoomInBtn.setAttribute("aria-label", "Zoom in");
    zoomInBtn.textContent = "+";

    controls.appendChild(zoomOutBtn);
    controls.appendChild(resetBtn);
    controls.appendChild(zoomInBtn);

    overlay.appendChild(stage);
    overlay.appendChild(closeBtn);
    overlay.appendChild(controls);
    document.body.appendChild(overlay);

    overlay._stage = stage;
    overlay._inner = inner;

    overlay.addEventListener("click", function (event) {
      // Only a click landing on the backdrop itself dismisses -- not one
      // that bubbles up from the stage, its content, or the controls.
      if (event.target === overlay) closeLightbox();
    });
    closeBtn.addEventListener("click", closeLightbox);
    zoomOutBtn.addEventListener("click", function () {
      stepScale(0.8);
    });
    zoomInBtn.addEventListener("click", function () {
      stepScale(1.25);
    });
    resetBtn.addEventListener("click", resetZoom);

    stage.addEventListener("wheel", onWheel, { passive: false });
    stage.addEventListener("mousedown", onMouseDown);

    return overlay;
  }

  function applyTransform() {
    var overlay = document.getElementById("mdview-lightbox");
    if (!overlay || !zoomState) return;
    overlay._inner.style.transform =
      "translate(" + zoomState.x + "px, " + zoomState.y + "px) scale(" + zoomState.scale + ")";
  }

  // Zooms about a point given in stage-local coordinates, keeping the
  // content under that point stationary on screen -- shared by wheel/pinch
  // and the +/- buttons (which zoom about the stage center).
  function zoomAbout(cx, cy, factor) {
    if (!zoomState) return;
    var newScale = clamp(zoomState.scale * factor, MIN_SCALE, MAX_SCALE);
    var ratio = newScale / zoomState.scale;
    zoomState.x = cx - (cx - zoomState.x) * ratio;
    zoomState.y = cy - (cy - zoomState.y) * ratio;
    zoomState.scale = newScale;
    applyTransform();
  }

  function stepScale(factor) {
    var overlay = document.getElementById("mdview-lightbox");
    if (!overlay || !zoomState) return;
    var rect = overlay._stage.getBoundingClientRect();
    zoomAbout(rect.width / 2, rect.height / 2, factor);
  }

  function resetZoom() {
    if (!zoomState) return;
    zoomState.scale = 1;
    zoomState.x = 0;
    zoomState.y = 0;
    applyTransform();
  }

  // A macOS trackpad pinch arrives as a wheel event with ctrlKey true; both
  // it and an ordinary wheel scroll are handled through this one path.
  // preventDefault() keeps the page behind from scrolling and keeps the
  // gesture from falling through to the app's own page-zoom.
  function onWheel(event) {
    if (!zoomState) return;
    event.preventDefault();
    var overlay = document.getElementById("mdview-lightbox");
    var rect = overlay._stage.getBoundingClientRect();
    var cx = event.clientX - rect.left;
    var cy = event.clientY - rect.top;
    var factor = Math.exp(-event.deltaY * 0.0015);
    zoomAbout(cx, cy, factor);
  }

  function onMouseDown(event) {
    if (!zoomState) return;
    event.preventDefault();
    dragState = {
      startX: event.clientX,
      startY: event.clientY,
      startPanX: zoomState.x,
      startPanY: zoomState.y,
    };
    var overlay = document.getElementById("mdview-lightbox");
    overlay._stage.classList.add("mdview-dragging");
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  }

  function onMouseMove(event) {
    if (!dragState || !zoomState) return;
    zoomState.x = dragState.startPanX + (event.clientX - dragState.startX);
    zoomState.y = dragState.startPanY + (event.clientY - dragState.startY);
    applyTransform();
  }

  function onMouseUp() {
    dragState = null;
    var overlay = document.getElementById("mdview-lightbox");
    if (overlay) overlay._stage.classList.remove("mdview-dragging");
    document.removeEventListener("mousemove", onMouseMove);
    document.removeEventListener("mouseup", onMouseUp);
  }

  function onKeyDown(event) {
    if (event.key === "Escape") closeLightbox();
  }

  function openLightbox(node) {
    var overlay = getLightbox();

    overlay._inner.innerHTML = "";
    var clone = node.cloneNode(true);
    clone.removeAttribute("data-mdview-zoom");
    overlay._inner.appendChild(clone);

    zoomState = { scale: 1, x: 0, y: 0 };
    applyTransform();

    savedScrollY = window.scrollY;
    savedBodyOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    overlay.hidden = false;
    document.addEventListener("keydown", onKeyDown);
  }

  function closeLightbox() {
    var overlay = document.getElementById("mdview-lightbox");
    if (!overlay || overlay.hidden) return;

    overlay.hidden = true;
    overlay._inner.innerHTML = "";
    zoomState = null;
    dragState = null;

    document.body.style.overflow = savedBodyOverflow;
    window.scrollTo(0, savedScrollY);
    document.removeEventListener("keydown", onKeyDown);
    document.removeEventListener("mousemove", onMouseMove);
    document.removeEventListener("mouseup", onMouseUp);
  }

  // Called on first load and again after every live-reload body swap.
  window.mdviewRenderAll = function () {
    renderMath();
    // mermaid.run() is asynchronous; renderDiagrams() always returns a
    // promise (resolved immediately when mermaid is absent or throws) so
    // enhanceZoomables() runs exactly once, after diagrams exist, and still
    // runs -- covering images -- even when mermaid itself failed.
    renderDiagrams().then(enhanceZoomables);
    renderSidebarBody();
  };

  // ---- Sidebar state management -------------------------------------------

  function postToHost(text) {
    try {
      window.webkit.messageHandlers.mdview.postMessage(text);
    } catch (err) {
      /* running outside the app (e.g. --print-html output opened in a
         browser): the page still works, it just cannot persist. */
    }
  }

  var sidebarTab = "outline";
  var bookmarks = [];

  function setSidebar(open, tab) {
    var sidebar = document.getElementById("mdview-sidebar");
    if (!sidebar) return;
    sidebarTab = tab || sidebarTab;
    sidebar.hidden = !open;
    var opener = document.getElementById("mdview-sidebar-open");
    if (opener) opener.hidden = open;
    var tabs = document.querySelectorAll(".mdview-tab");
    for (var i = 0; i < tabs.length; i++) {
      tabs[i].setAttribute(
        "aria-selected",
        tabs[i].getAttribute("data-tab") === sidebarTab ? "true" : "false"
      );
    }
    renderSidebarBody();
    postToHost("setSidebar:" + (open ? "1" : "0") + ":" + sidebarTab);
  }

  window.mdviewSetBookmarks = function (items, starred) {
    bookmarks = items || [];
    var star = document.getElementById("mdview-star");
    if (star) {
      star.textContent = starred ? "★" : "☆";
      star.setAttribute("aria-pressed", starred ? "true" : "false");
    }
    if (sidebarTab === "bookmarks") renderSidebarBody();
  };

  // Turn heading text into a URL-safe id. Duplicates get a numeric suffix so
  // two "## Setup" headings still scroll to different places.
  function slugify(text, seen) {
    var base = text
      .toLowerCase()
      .replace(/[^\w\s-]/g, "")
      .trim()
      .replace(/\s+/g, "-");
    if (!base) base = "section";
    var slug = base;
    var n = 2;
    while (seen[slug]) {
      slug = base + "-" + n;
      n++;
    }
    seen[slug] = true;
    return slug;
  }

  // Build the outline from the rendered document. Rebuilt wholesale on every
  // render rather than patched: incremental updates over already-processed
  // nodes are the defect class that has bitten this project twice.
  function buildOutline() {
    var content = document.getElementById("mdview-content");
    if (!content) return "<p class=\"mdview-sidebar-empty\">No document.</p>";
    var headings = content.querySelectorAll("h1, h2, h3, h4, h5, h6");
    if (!headings.length) {
      return "<p class=\"mdview-sidebar-empty\">No headings.</p>";
    }
    var seen = {};
    var parts = ["<ul>"];
    for (var i = 0; i < headings.length; i++) {
      var h = headings[i];
      if (!h.id) h.id = slugify(h.textContent, seen);
      var level = parseInt(h.tagName.substring(1), 10);
      parts.push(
        '<li style="padding-left:' + ((level - 1) * 0.6) + 'rem">' +
        '<a href="#" data-outline-id="' + h.id + '"></a></li>'
      );
      // textContent is assigned below rather than interpolated, so heading
      // text can never inject markup into the sidebar.
    }
    parts.push("</ul>");
    var html = parts.join("");
    var host = document.createElement("div");
    host.innerHTML = html;
    var links = host.querySelectorAll("a[data-outline-id]");
    for (var j = 0; j < links.length; j++) {
      links[j].textContent = headings[j].textContent;
    }
    return host.innerHTML;
  }

  // Task 7 replaces the outline branch; Task 8 the bookmarks branch.
  function renderSidebarBody() {
    var body = document.getElementById("mdview-sidebar-body");
    if (!body) return;
    if (sidebarTab === "outline") {
      body.innerHTML = buildOutline();
      var links = body.querySelectorAll("a[data-outline-id]");
      for (var i = 0; i < links.length; i++) {
        links[i].addEventListener("click", function (event) {
          event.preventDefault();
          var target = document.getElementById(
            event.currentTarget.getAttribute("data-outline-id")
          );
          if (target) target.scrollIntoView({ behavior: "smooth", block: "start" });
        });
      }
    } else {
      if (!bookmarks.length) {
        body.innerHTML = "<p class=\"mdview-sidebar-empty\">No bookmarks yet.</p>";
        return;
      }
      body.innerHTML = "<ul></ul>";
      var list = body.firstChild;
      for (var k = 0; k < bookmarks.length; k++) {
        (function (entry) {
          var li = document.createElement("li");
          var a = document.createElement("a");
          a.href = "#";
          a.textContent = entry.name;   // textContent, never innerHTML
          a.title = entry.path;
          a.addEventListener("click", function (event) {
            event.preventDefault();
            postToHost("openPath:" + entry.path);
          });
          li.appendChild(a);
          list.appendChild(li);
        })(bookmarks[k]);
      }
    }
  }

  window.mdviewSetSidebar = setSidebar;

  // ---- Sidebar event listeners (attach once at DOMContentLoaded) ----------
  //
  // The sidebar markup is OUTSIDE #mdview-content and is therefore not
  // recreated by live reload. These listeners must attach exactly once here,
  // not inside mdviewRenderAll (which runs again on every save).

  function attachSidebarListeners() {
    var closeBtn = document.getElementById("mdview-sidebar-close");
    if (closeBtn) {
      closeBtn.addEventListener("click", function () {
        setSidebar(false, sidebarTab);
      });
    }

    var opener = document.getElementById("mdview-sidebar-open");
    if (opener) {
      opener.addEventListener("click", function () {
        setSidebar(true, sidebarTab);
      });
    }

    var themeBtn = document.getElementById("mdview-theme");
    if (themeBtn) {
      themeBtn.addEventListener("click", function () {
        // The cycle order (System -> Light -> Dark) is defined exactly once,
        // in Rust's Theme::next, which also drives the ⌘T shortcut. Posting
        // a bare "cycleTheme" message and letting the host apply it there
        // means this button and ⌘T can never disagree about the order.
        postToHost("cycleTheme");
      });
    }

    var starBtn = document.getElementById("mdview-star");
    if (starBtn) {
      starBtn.addEventListener("click", function () {
        postToHost("toggleBookmark");
      });
    }

    var tabs = document.querySelectorAll(".mdview-tab");
    for (var i = 0; i < tabs.length; i++) {
      tabs[i].addEventListener("click", function (event) {
        var tab = event.target.getAttribute("data-tab");
        if (tab) setSidebar(true, tab);
      });
    }
  }

  document.addEventListener("DOMContentLoaded", function () {
    attachSidebarListeners();
    window.mdviewRenderAll();
  });
})();
