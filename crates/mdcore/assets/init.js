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
  var SIDEBAR_WIDTH_MIN = 160;
  var SIDEBAR_WIDTH_MAX = 600;
  var SIDEBAR_WIDTH_DEFAULT = 260;
  var sidebarWidth = SIDEBAR_WIDTH_DEFAULT;
  var sidebarResizeState = null;

  function clampSidebarWidth(px) {
    return Math.min(SIDEBAR_WIDTH_MAX, Math.max(SIDEBAR_WIDTH_MIN, px));
  }

  function applySidebarWidth(px) {
    var w = clampSidebarWidth(px);
    sidebarWidth = w;
    document.documentElement.style.setProperty("--sidebar-width", w + "px");
    return w;
  }

  window.mdviewSetSidebarWidth = function (px) {
    var n = Number(px);
    if (!isFinite(n)) return;
    applySidebarWidth(n);
  };

  function setSidebar(open, tab) {
    var sidebar = document.getElementById("mdview-sidebar");
    if (!sidebar) return;
    sidebarTab = tab || sidebarTab;
    sidebar.hidden = !open;
    var resizer = document.getElementById("mdview-sidebar-resizer");
    if (resizer) resizer.hidden = !open;
    document.documentElement.setAttribute("data-sidebar-open", open ? "1" : "0");
    var toggle = document.getElementById("mdview-sidebar-toggle");
    if (toggle) toggle.setAttribute("aria-expanded", open ? "true" : "false");
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

  function onSidebarResizerPointerDown(event) {
    var sidebar = document.getElementById("mdview-sidebar");
    var resizer = document.getElementById("mdview-sidebar-resizer");
    if (!sidebar || !resizer || sidebar.hidden) return;
    if (event.button != null && event.button !== 0) return;
    event.preventDefault();
    var rect = sidebar.getBoundingClientRect();
    sidebarResizeState = {
      startX: event.clientX,
      startWidth: rect.width || sidebarWidth,
    };
    document.body.classList.add("mdview-resizing");
    document.addEventListener("mousemove", onSidebarResizerPointerMove);
    document.addEventListener("mouseup", onSidebarResizerPointerUp);
    document.addEventListener("pointermove", onSidebarResizerPointerMove);
    document.addEventListener("pointerup", onSidebarResizerPointerUp);
  }

  function onSidebarResizerPointerMove(event) {
    if (!sidebarResizeState) return;
    var dx = sidebarResizeState.startX - event.clientX;
    var next = clampSidebarWidth(sidebarResizeState.startWidth + dx);
    document.documentElement.style.setProperty("--sidebar-width", next + "px");
    sidebarWidth = next;
  }

  function onSidebarResizerPointerUp(event) {
    if (!sidebarResizeState) return;
    var dx = sidebarResizeState.startX - event.clientX;
    var next = clampSidebarWidth(sidebarResizeState.startWidth + dx);
    document.removeEventListener("mousemove", onSidebarResizerPointerMove);
    document.removeEventListener("mouseup", onSidebarResizerPointerUp);
    document.removeEventListener("pointermove", onSidebarResizerPointerMove);
    document.removeEventListener("pointerup", onSidebarResizerPointerUp);
    document.body.classList.remove("mdview-resizing");
    sidebarResizeState = null;
    applySidebarWidth(next);
    postToHost("setSidebarWidth:" + next);
  }

  window.mdviewSetBookmarks = function (items, starred) {
    bookmarks = items || [];
    var star = document.getElementById("mdview-star");
    if (star) {
      // aria-pressed drives the fill in CSS; never write textContent here,
      // which would replace the inline SVG with a bare glyph.
      star.setAttribute("aria-pressed", starred ? "true" : "false");
      star.setAttribute(
        "aria-label",
        starred ? "Remove bookmark" : "Bookmark this document"
      );
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

  // ---- Document options ---------------------------------------------------
  function syncOptions() {
    var root = document.documentElement;
    var diff = root.getAttribute("data-view") === "diff";
    var menu = document.getElementById("mdview-options-menu");
    var viewToggle = document.getElementById("mdview-view-toggle");
    var layoutControls = document.getElementById("mdview-diff-layout-controls");
    var fullWidthToggle = document.getElementById("mdview-fullwidth-toggle");
    if (viewToggle) {
      viewToggle.textContent = diff ? "Show Markdown" : "Show Diff";
      viewToggle.setAttribute("aria-pressed", diff ? "true" : "false");
      viewToggle.disabled = !diff && !window.mdviewDiffAvailable;
    }
    if (layoutControls) layoutControls.hidden = !diff;
    var layout = root.getAttribute("data-diff-layout") || "unified";
    var choices = document.querySelectorAll(".mdview-layout-choice");
    for (var i = 0; i < choices.length; i++) {
      var selected = choices[i].getAttribute("data-layout") === layout;
      choices[i].setAttribute("data-selected", selected ? "true" : "false");
      choices[i].setAttribute("aria-pressed", selected ? "true" : "false");
    }
    if (fullWidthToggle) {
      var full = root.getAttribute("data-fullwidth") === "1";
      fullWidthToggle.textContent = full ? "Exit Full Width" : "Full Width";
      fullWidthToggle.setAttribute("aria-pressed", full ? "true" : "false");
    }
    if (menu && !diff) {
      // Keep the menu open when switching back to Markdown; the user can see
      // the available controls without an extra click.
    }
  }

  window.mdviewDiffAvailable = false;
  window.mdviewSetDiffAvailability = function (available) {
    window.mdviewDiffAvailable = !!available;
    syncOptions();
  };
  window.mdviewSetViewState = function (view, layout, fullWidth, available) {
    var root = document.documentElement;
    if (view === "diff") root.setAttribute("data-view", "diff");
    else root.removeAttribute("data-view");
    if (layout) root.setAttribute("data-diff-layout", layout);
    if (fullWidth) root.setAttribute("data-fullwidth", "1");
    else root.removeAttribute("data-fullwidth");
    if (typeof available === "boolean") window.mdviewDiffAvailable = available;
    syncOptions();
  };

  function attachOptionsListeners() {
    var toggle = document.getElementById("mdview-options-toggle");
    var menu = document.getElementById("mdview-options-menu");
    if (!toggle || !menu) return;
    toggle.addEventListener("click", function (event) {
      event.stopPropagation();
      menu.hidden = !menu.hidden;
      toggle.setAttribute("aria-expanded", menu.hidden ? "false" : "true");
    });
    var viewToggle = document.getElementById("mdview-view-toggle");
    if (viewToggle) viewToggle.addEventListener("click", function () { postToHost("toggleDiff"); });
    var choices = document.querySelectorAll(".mdview-layout-choice");
    for (var i = 0; i < choices.length; i++) {
      (function (choice) {
        choice.addEventListener("click", function () {
          postToHost("setDiffLayout:" + choice.getAttribute("data-layout"));
        });
      })(choices[i]);
    }
    var fullWidthToggle = document.getElementById("mdview-fullwidth-toggle");
    if (fullWidthToggle) fullWidthToggle.addEventListener("click", function () { postToHost("toggleFullWidth"); });
    document.addEventListener("click", function (event) {
      if (!menu.hidden && !menu.contains(event.target) && event.target !== toggle) {
        menu.hidden = true;
        toggle.setAttribute("aria-expanded", "false");
      }
    });
    syncOptions();
    if (typeof MutationObserver !== "undefined") {
      new MutationObserver(syncOptions).observe(document.documentElement, { attributes: true, attributeFilter: ["data-view", "data-diff-layout", "data-fullwidth"] });
    }
  }

  // ---- Sidebar event listeners (attach once at DOMContentLoaded) ----------
  //
  // The sidebar markup is OUTSIDE #mdview-content and is therefore not
  // recreated by live reload. These listeners must attach exactly once here,
  // not inside mdviewRenderAll (which runs again on every save).

  // The theme the page was built with. Previews are transient; this is what
  // the picker reverts to when the pointer leaves it without a click.
  var savedTheme = document.documentElement.getAttribute("data-theme") || "system";
  var savedDark = document.documentElement.getAttribute("data-dark");

  // Apply a theme without rebuilding the page. Rust emits every theme's chrome
  // block scoped to :root[data-theme=…] and every syntect sheet behind its own
  // id, so switching is an attribute flip. Mermaid keeps the colours it was
  // drawn with -- re-rendering diagrams on hover is what made theme changes
  // feel slow before, and the commit path reloads and redraws them anyway.
  function applyTheme(themeId, dark) {
    var root = document.documentElement;
    if (!themeId || themeId === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", themeId);
    if (dark === "0" || dark === "1") root.setAttribute("data-dark", dark);
    else root.removeAttribute("data-dark");

    // Syntect emits full rulesets, which cannot be scoped by attribute, so the
    // sheets are selected by toggling `media` instead.
    var sheets = document.querySelectorAll('style[id^="mdview-hl-"]');
    for (var i = 0; i < sheets.length; i++) {
      var id = sheets[i].id;
      if (id === "mdview-hl-light") {
        sheets[i].media = themeId && themeId !== "system" ? "not all" : "all";
      } else if (id === "mdview-hl-dark") {
        sheets[i].media =
          themeId && themeId !== "system" ? "not all" : "(prefers-color-scheme: dark)";
      } else {
        sheets[i].media = id === "mdview-hl-" + themeId ? "all" : "not all";
      }
    }
  }

  function attachSidebarListeners() {
    var resizerEl = document.getElementById("mdview-sidebar-resizer");
    if (resizerEl) {
      resizerEl.addEventListener("mousedown", onSidebarResizerPointerDown);
      resizerEl.addEventListener("pointerdown", onSidebarResizerPointerDown);
      resizerEl.addEventListener("keydown", function (event) {
        if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
          event.preventDefault();
          var step = event.key === "ArrowLeft" ? 20 : -20;
          var next = clampSidebarWidth(sidebarWidth + step);
          applySidebarWidth(next);
          postToHost("setSidebarWidth:" + next);
        }
      });
      if (!resizerEl.hasAttribute("tabindex")) resizerEl.setAttribute("tabindex", "0");
    }

    var toggle = document.getElementById("mdview-sidebar-toggle");
    if (toggle) {
      toggle.addEventListener("click", function () {
        var sidebar = document.getElementById("mdview-sidebar");
        if (sidebar) setSidebar(sidebar.hidden, sidebarTab);
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
      // Bind the button itself: the tabs hold an <svg>, so event.target is the
      // icon (or a line inside it) and carries no data-tab.
      (function (button) {
        button.addEventListener("click", function () {
          var tab = button.getAttribute("data-tab");
          if (tab) setSidebar(true, tab);
        });
      })(tabs[i]);
    }

    var picker = document.getElementById("mdview-theme");
    var themeItems = document.querySelectorAll(".mdview-theme-item");
    for (var j = 0; j < themeItems.length; j++) {
      (function (item) {
        // Hovering previews; only a click commits. The preview is a local
        // attribute flip, so it costs nothing and needs no round trip.
        item.addEventListener("mouseenter", function () {
          applyTheme(
            item.getAttribute("data-theme-id"),
            item.getAttribute("data-theme-dark")
          );
        });
        item.addEventListener("click", function () {
          var themeId = item.getAttribute("data-theme-id");
          if (themeId) {
            // Adopt it as the theme to revert to, so the pending reload does
            // not race the picker closing and snap back to the old one.
            savedTheme = themeId;
            savedDark = item.getAttribute("data-theme-dark");
            if (picker) picker.open = false;
            postToHost("setTheme:" + themeId + ":" + Math.round(window.scrollY));
          }
        });
      })(themeItems[j]);
    }

    if (picker) {
      picker.addEventListener("mouseleave", function () {
        applyTheme(savedTheme, savedDark);
        picker.open = false;
      });
    }
  }

  document.addEventListener("DOMContentLoaded", function () {
    attachSidebarListeners();
    attachOptionsListeners();
    window.mdviewRenderAll();
  });
})();
