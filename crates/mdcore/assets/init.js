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

    // Clicking the diagram/image opens the overlay -- except when the author
    // wrapped it in a link (e.g. [![alt](img)](https://...)), where the click
    // should follow the link instead. `z` is what reaches the overlay in that
    // case, now that the badge that used to is gone.
    wrapper.addEventListener("click", function () {
      var content = document.getElementById("mdview-content");
      var link = wrapper.closest("a");
      if (link && content && content.contains(link)) return;
      openLightbox(node);
    });
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

  // Pan by a whole nudge rather than a pixel: the arrow keys are for moving
  // around a zoomed-in diagram, not for fine positioning (that is the drag).
  var LIGHTBOX_PAN = 60;

  function panBy(dx, dy) {
    if (!zoomState) return;
    zoomState.x += dx;
    zoomState.y += dy;
    applyTransform();
  }

  function onKeyDown(event) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    switch (event.key) {
      case "Escape":
        closeLightbox();
        return;
      // "=" as well as "+": zoom in is the unshifted key on a US layout.
      case "+":
      case "=":
        stepScale(1.25);
        break;
      case "-":
        stepScale(0.8);
        break;
      case "0":
        resetZoom();
        break;
      // An arrow moves the VIEW, so the content travels the other way.
      case "ArrowLeft":
        panBy(LIGHTBOX_PAN, 0);
        break;
      case "ArrowRight":
        panBy(-LIGHTBOX_PAN, 0);
        break;
      case "ArrowUp":
        panBy(0, LIGHTBOX_PAN);
        break;
      case "ArrowDown":
        panBy(0, -LIGHTBOX_PAN);
        break;
      default:
        return;
    }
    event.preventDefault();
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
    refreshFind();
  };

  // ---- Find in page --------------------------------------------------------
  //
  // Matches are wrapped in <mark> elements inside #mdview-content and unwrapped
  // again on close, so a closed find bar leaves the document DOM exactly as it
  // was. Live reload replaces #mdview-content wholesale, which throws the marks
  // away with it -- mdviewRenderAll re-runs the search rather than trying to
  // patch surviving marks back together.

  // A cap, not a limit on what is findable: a one-character query over a large
  // document would otherwise wrap tens of thousands of nodes for a count no one
  // reads. Navigation still works over everything up to the cap.
  var FIND_MATCH_LIMIT = 2000;
  var findQuery = "";
  var findMatches = [];
  var findIndex = -1;

  function findBarEl() { return document.getElementById("mdview-find"); }
  function findInputEl() { return document.getElementById("mdview-find-input"); }

  function findIsOpen() {
    var bar = findBarEl();
    return !!bar && !bar.hidden;
  }

  // Text worth searching: inside the document body only, and never inside an
  // <svg> (a <mark> there renders nothing and corrupts a mermaid diagram) or
  // inside KaTeX's hidden MathML copy, which repeats every formula's source and
  // would count each match twice.
  function collectFindTextNodes(root) {
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: function (node) {
        if (!node.nodeValue) return NodeFilter.FILTER_REJECT;
        var el = node.parentNode;
        while (el && el !== root) {
          var tag = el.tagName ? el.tagName.toLowerCase() : "";
          if (tag === "svg" || tag === "script" || tag === "style") {
            return NodeFilter.FILTER_REJECT;
          }
          if (el.classList && el.classList.contains("katex-mathml")) {
            return NodeFilter.FILTER_REJECT;
          }
          el = el.parentNode;
        }
        return NodeFilter.FILTER_ACCEPT;
      },
    });
    var nodes = [];
    var node;
    while ((node = walker.nextNode())) nodes.push(node);
    return nodes;
  }

  function clearFindHighlights() {
    for (var i = 0; i < findMatches.length; i++) {
      var mark = findMatches[i];
      var parent = mark.parentNode;
      if (!parent) continue;
      parent.replaceChild(document.createTextNode(mark.textContent), mark);
      // Re-join the split halves, or a later search would never see a match
      // that straddles the seam left behind by this one.
      parent.normalize();
    }
    findMatches = [];
    findIndex = -1;
  }

  function highlightFindMatches(query) {
    clearFindHighlights();
    var content = document.getElementById("mdview-content");
    if (!content || !query) return;
    var needle = query.toLowerCase();
    var nodes = collectFindTextNodes(content);
    for (var i = 0; i < nodes.length && findMatches.length < FIND_MATCH_LIMIT; i++) {
      var node = nodes[i];
      var at = node.nodeValue.toLowerCase().indexOf(needle);
      while (at >= 0 && findMatches.length < FIND_MATCH_LIMIT) {
        // splitText leaves `hit` holding exactly the match and `node` holding
        // the remainder, so the next indexOf runs against fresh offsets.
        var hit = node.splitText(at);
        node = hit.splitText(needle.length);
        var mark = document.createElement("mark");
        mark.className = "mdview-find-hit";
        hit.parentNode.replaceChild(mark, hit);
        mark.appendChild(hit);
        findMatches.push(mark);
        at = node.nodeValue.toLowerCase().indexOf(needle);
      }
    }
  }

  function updateFindCount() {
    var label = document.getElementById("mdview-find-count");
    if (!label) return;
    var none = !findMatches.length;
    if (!findQuery) {
      label.textContent = "";
    } else if (none) {
      label.textContent = "No results";
    } else {
      label.textContent =
        findIndex + 1 + " of " + findMatches.length +
        (findMatches.length >= FIND_MATCH_LIMIT ? "+" : "");
    }
  }

  function setFindIndex(next) {
    if (!findMatches.length) {
      findIndex = -1;
      updateFindCount();
      return;
    }
    var count = findMatches.length;
    // Wrap in both directions: (-1 % n) is -1 in JS, not n - 1.
    findIndex = ((next % count) + count) % count;
    for (var i = 0; i < count; i++) {
      if (i === findIndex) findMatches[i].classList.add("is-current");
      else findMatches[i].classList.remove("is-current");
    }
    var current = findMatches[findIndex];
    if (current.scrollIntoView) {
      current.scrollIntoView({ block: "center", inline: "nearest" });
    }
    updateFindCount();
  }

  // `keepPosition` holds the user's place across a live reload; a fresh query
  // always starts at the first match.
  function runFind(query, keepPosition) {
    var previous = findIndex;
    findQuery = query || "";
    highlightFindMatches(findQuery);
    if (!findMatches.length) {
      findIndex = -1;
      updateFindCount();
      return;
    }
    setFindIndex(keepPosition && previous >= 0 ? previous : 0);
  }

  function stepFind(delta) {
    if (!findQuery) {
      window.mdviewOpenFind();
      return;
    }
    // ⌘G after the bar was closed re-runs the last query rather than doing
    // nothing: the marks are gone, but the query is still what the user wants.
    if (!findMatches.length) {
      runFind(findQuery);
      return;
    }
    setFindIndex(findIndex + delta);
  }

  window.mdviewOpenFind = function () {
    var bar = findBarEl();
    var input = findInputEl();
    if (!bar || !input) return;
    // Seed from the document selection, the way Find does across macOS. A
    // selection inside the bar itself is the user's own query, not a seed.
    var selection = "";
    try {
      selection = String(window.getSelection() || "").trim();
    } catch (err) {
      /* no selection API: fall through with the previous query */
    }
    if (selection && selection.length <= 200 && !bar.contains(document.activeElement)) {
      input.value = selection;
    }
    bar.hidden = false;
    input.focus();
    input.select();
    if (input.value !== findQuery || !findMatches.length) runFind(input.value);
    else updateFindCount();
  };

  window.mdviewCloseFind = function () {
    var bar = findBarEl();
    if (!bar) return;
    bar.hidden = true;
    clearFindHighlights();
    findQuery = "";
    updateFindCount();
    // Hand the keyboard back to the document, or the next arrow key would
    // still be editing a field the user can no longer see.
    var input = findInputEl();
    if (input) input.blur();
  };

  window.mdviewFindNext = function () { stepFind(1); };
  window.mdviewFindPrevious = function () { stepFind(-1); };

  // Called after every body swap: the marks went out with the old innerHTML.
  function refreshFind() {
    if (!findIsOpen() || !findQuery) return;
    findMatches = [];
    runFind(findQuery, true);
  }

  function attachFindListeners() {
    var input = findInputEl();
    if (input) {
      input.addEventListener("input", function () {
        runFind(input.value);
      });
      input.addEventListener("keydown", function (event) {
        if (event.key === "Enter") {
          event.preventDefault();
          stepFind(event.shiftKey ? -1 : 1);
          // Hand the keyboard back to the document, the way /pattern<CR> does
          // in a pager: n and N then STEP through the matches instead of being
          // typed into the box. The bar stays up with its highlights, and / or
          // ⌘F brings the field back with the query selected for editing.
          input.blur();
        } else if (event.key === "Escape") {
          event.preventDefault();
          window.mdviewCloseFind();
        }
      });
    }
    // In the app the Edit > Find menu items own these shortcuts and AppKit
    // never lets them reach the page. This is what makes find work in a plain
    // browser, where `--print-html` output is opened with no menu bar at all.
    document.addEventListener("keydown", function (event) {
      if (!event.metaKey || event.ctrlKey || event.altKey) {
        // Escape closes the bar, but not while the lightbox is open -- that
        // overlay is on top and owns the key.
        if (event.key === "Escape" && findIsOpen() && !zoomState) {
          window.mdviewCloseFind();
        }
        return;
      }
      var key = (event.key || "").toLowerCase();
      if (key === "f") {
        event.preventDefault();
        window.mdviewOpenFind();
      } else if (key === "g") {
        event.preventDefault();
        stepFind(event.shiftKey ? -1 : 1);
      }
    });
  }

  // ---- Sidebar state management -------------------------------------------

  // Returns false when there is no host listening -- running outside the app
  // (e.g. --print-html output opened in a browser). The page still works; the
  // callers that have a purely local equivalent fall back to it.
  function postToHost(text) {
    try {
      window.webkit.messageHandlers.mdview.postMessage(text);
      return true;
    } catch (err) {
      return false;
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
    // With the tabs gone, this heading is the only thing saying which of the
    // two panels you are looking at.
    var title = document.getElementById("mdview-sidebar-title");
    if (title) title.textContent = sidebarTab === "bookmarks" ? "Bookmarks" : "Outline";
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

  // The star button was the whole feedback for bookmarking. With it gone,
  // pressing m with the sidebar shut would be a keypress into the void, so the
  // change is reported instead. The first call is the page being told what it
  // loaded with, which is not news.
  var bookmarksKnown = false;
  var currentIsBookmarked = false;

  window.mdviewSetBookmarks = function (items, starred) {
    bookmarks = items || [];
    var next = !!starred;
    if (bookmarksKnown && next !== currentIsBookmarked) {
      showNote(next ? "Bookmarked" : "Bookmark removed");
    }
    currentIsBookmarked = next;
    bookmarksKnown = true;
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
  window.mdviewDiffAvailable = false;
  window.mdviewSetDiffAvailability = function (available) {
    window.mdviewDiffAvailable = !!available;
  };
  window.mdviewSetViewState = function (view, layout, fullWidth, available) {
    var root = document.documentElement;
    if (view === "diff") root.setAttribute("data-view", "diff");
    else root.removeAttribute("data-view");
    if (layout) root.setAttribute("data-diff-layout", layout);
    if (fullWidth) root.setAttribute("data-fullwidth", "1");
    else root.removeAttribute("data-fullwidth");
    if (typeof available === "boolean") window.mdviewDiffAvailable = available;
  };

  // ---- Theme palette --------------------------------------------------------
  //
  // Replaces the picker that used to live in the sidebar header. Appended to
  // document.body, outside #mdview-content, for the reason the lightbox and the
  // cheat sheet are: live reload replaces that div's innerHTML and would take
  // an overlay living inside it with it.
  //
  // The reason it exists rather than deferring to View > Theme in the menu bar:
  // a native menu cannot show you a theme before you commit to it, and seeing
  // the document in a palette is the whole point of choosing one.

  var themeRows = [];      // every row, in menu order
  var themeMatches = [];   // the rows currently passing the filter
  var themeIndex = -1;     // which of themeMatches is highlighted

  function themePaletteEl() {
    return document.getElementById("mdview-theme-palette");
  }

  function themePaletteInput() {
    return document.getElementById("mdview-theme-search");
  }

  function themePaletteIsOpen() {
    var el = themePaletteEl();
    return !!el && !el.hidden;
  }

  // The themes come from the page itself: Rust emits a chrome block per theme,
  // so the list can be read back off the stylesheet rather than duplicated in
  // JS and left to drift.
  function themeCatalogue() {
    if (window.mdviewThemes && window.mdviewThemes.length) return window.mdviewThemes;
    return [];
  }

  function buildThemePalette() {
    var overlay = document.createElement("div");
    overlay.id = "mdview-theme-palette";
    overlay.hidden = true;
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-label", "Theme");

    var panel = document.createElement("div");
    panel.className = "mdview-palette-panel";

    var input = document.createElement("input");
    input.type = "text";
    input.id = "mdview-theme-search";
    input.className = "mdview-palette-search";
    input.placeholder = "Theme";
    input.setAttribute("aria-label", "Search themes");
    input.setAttribute("autocomplete", "off");
    input.setAttribute("autocorrect", "off");
    input.setAttribute("spellcheck", "false");
    panel.appendChild(input);

    var list = document.createElement("div");
    list.className = "mdview-palette-list";
    list.id = "mdview-theme-list";
    list.setAttribute("role", "listbox");
    panel.appendChild(list);

    var empty = document.createElement("p");
    empty.className = "mdview-palette-empty";
    empty.id = "mdview-theme-empty";
    empty.textContent = "No themes match.";
    empty.hidden = true;
    panel.appendChild(empty);

    overlay.appendChild(panel);
    document.body.appendChild(overlay);

    themeRows = [];
    var catalogue = themeCatalogue();
    for (var i = 0; i < catalogue.length; i++) {
      (function (entry) {
        var row = document.createElement("button");
        row.type = "button";
        row.className = "mdview-palette-row";
        row.setAttribute("role", "option");
        row.setAttribute("data-theme-id", entry.id);
        row.setAttribute("data-theme-dark", entry.dark);
        row.textContent = entry.label;   // textContent, never innerHTML
        // Hover previews and a click commits, the same two gestures the old
        // picker had.
        row.addEventListener("mouseenter", function () {
          highlightTheme(themeMatches.indexOf(row));
        });
        row.addEventListener("click", function () {
          commitTheme(row);
        });
        list.appendChild(row);
        themeRows.push(row);
      })(catalogue[i]);
    }

    overlay.addEventListener("click", function (event) {
      // Only the backdrop dismisses, not a click bubbling out of the panel.
      if (event.target === overlay) closeThemePalette(true);
    });
    input.addEventListener("input", function () {
      filterThemes(input.value);
    });
    return overlay;
  }

  function highlightTheme(index) {
    if (!themeMatches.length) {
      themeIndex = -1;
      return;
    }
    var count = themeMatches.length;
    themeIndex = ((index % count) + count) % count;
    for (var i = 0; i < themeRows.length; i++) {
      themeRows[i].classList.remove("is-current");
      themeRows[i].setAttribute("aria-selected", "false");
    }
    var row = themeMatches[themeIndex];
    row.classList.add("is-current");
    row.setAttribute("aria-selected", "true");
    if (row.scrollIntoView) row.scrollIntoView({ block: "nearest" });
    // Moving the highlight previews, exactly as hovering the old picker did.
    // This is the thing the native menu cannot do.
    applyTheme(row.getAttribute("data-theme-id"), row.getAttribute("data-theme-dark"));
  }

  function filterThemes(query) {
    var needle = (query || "").toLowerCase().trim();
    themeMatches = [];
    for (var i = 0; i < themeRows.length; i++) {
      var hit = !needle || themeRows[i].textContent.toLowerCase().indexOf(needle) >= 0;
      themeRows[i].hidden = !hit;
      if (hit) themeMatches.push(themeRows[i]);
    }
    var empty = document.getElementById("mdview-theme-empty");
    if (empty) empty.hidden = themeMatches.length > 0;
    // A narrowed list starts at its first match; leaving the highlight where it
    // was would preview a theme no longer on screen.
    if (themeMatches.length) highlightTheme(0);
    else {
      themeIndex = -1;
      // Nothing matches, so nothing is being previewed: put the page back.
      applyTheme(savedTheme, savedDark);
    }
  }

  function commitTheme(row) {
    if (!row) return;
    var themeId = row.getAttribute("data-theme-id");
    if (!themeId) return;
    // Adopt it as the theme to revert to, so closing the palette cannot snap
    // back to the old one while the reload is still in flight.
    savedTheme = themeId;
    savedDark = row.getAttribute("data-theme-dark");
    closeThemePalette(false);
    postToHost("setTheme:" + themeId + ":" + Math.round(window.scrollY));
  }

  function closeThemePalette(revert) {
    var overlay = themePaletteEl();
    if (!overlay) return;
    // esc puts back whatever the previewing was standing on top of; enter has
    // already adopted its choice as `savedTheme`, so the same call is a no-op.
    if (revert) applyTheme(savedTheme, savedDark);
    overlay.hidden = true;
    var input = themePaletteInput();
    if (input) input.blur();
  }

  function openThemePalette() {
    var overlay = themePaletteEl() || buildThemePalette();
    overlay.hidden = false;
    var input = themePaletteInput();
    if (input) {
      input.value = "";
      input.focus();
    }
    filterThemes("");
    // Start on the theme in use, not the top of the list, so the palette opens
    // showing you where you already are.
    for (var i = 0; i < themeMatches.length; i++) {
      if (themeMatches[i].getAttribute("data-theme-id") === savedTheme) {
        highlightTheme(i);
        return;
      }
    }
  }

  function toggleThemePalette() {
    if (themePaletteIsOpen()) closeThemePalette(true);
    else openThemePalette();
  }

  // Driven from the document handler rather than from the search field, so it
  // works wherever the focus actually is -- a row clicked with the mouse takes
  // focus off the input, and the arrows have to keep steering the list.
  function onThemePaletteKey(event) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        closeThemePalette(true);
        break;
      case "Enter":
        event.preventDefault();
        commitTheme(themeMatches[themeIndex]);
        break;
      case "ArrowDown":
        event.preventDefault();
        highlightTheme(themeIndex + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        highlightTheme(themeIndex - 1);
        break;
      default:
        break;
    }
  }

  // ---- The first-run hint ---------------------------------------------------
  //
  // Shown once ever, queued by the app. Its own element rather than a banner:
  // banners are drawn from the warn palette and say something is wrong, and
  // giving them a fade would put every real warning at risk of vanishing.

  var HINT_LINGER_MS = 6000;

  function dismissHint() {
    var hint = document.getElementById("mdview-hint");
    if (!hint) return;
    hint.classList.add("is-leaving");
    // Let the fade finish before the node goes, or it would blink out.
    setTimeout(function () {
      if (hint.parentNode) hint.parentNode.removeChild(hint);
    }, 400);
  }

  window.mdviewShowShortcutsHint = function () {
    if (document.getElementById("mdview-hint")) return;
    var hint = document.createElement("div");
    hint.id = "mdview-hint";
    hint.setAttribute("role", "status");
    hint.textContent = "Press ? for keyboard shortcuts";
    document.body.appendChild(hint);
    // Two frames, so the element is laid out before the transition starts;
    // adding the class in the same frame would skip the fade in.
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        hint.classList.add("is-visible");
      });
    });
    setTimeout(dismissHint, HINT_LINGER_MS);
  };

  // ---- Keyboard shortcuts -------------------------------------------------
  //
  // Single-key, vim-flavoured bindings. They are safe here because the page
  // has exactly one text field (the find input) and no editing at all, so an
  // unmodified letter can never be something the user meant to type.
  //
  // ONE table drives both the dispatcher and the `?` cheat sheet. A binding
  // that exists but is undocumented is therefore not expressible -- which is
  // the whole reason the table exists, since a single-key shortcut leaves no
  // trace in the menu bar to discover it by.

  var SCROLL_LINE = 60;
  // Two lines of overlap between pages, so nothing is stepped over unread.
  var PAGE_OVERLAP = 2 * SCROLL_LINE;
  // A "gg" is two presses of g within this window; a lone g does nothing.
  var G_CHORD_MS = 700;
  // How long a heading jump stays chainable. A second press inside the window
  // steps from the heading the last jump was HEADING FOR, not from wherever
  // the smooth scroll has reached -- without this, "]]]" pressed quickly reads
  // an intermediate position three times and lands one heading along.
  var HEADING_CHAIN_MS = 700;
  // Breathing room above a heading the jump lands on.
  var HEADING_MARGIN = 12;
  // A heading nearer the top than this is the one you are standing on rather
  // than one to jump to. It must exceed HEADING_MARGIN, or the heading a jump
  // just landed on would read as "next" again and the second press would stall.
  var HEADING_EPSILON = HEADING_MARGIN + 4;

  var pendingG = 0;           // timestamp of an unconsumed "g"
  var lastHeadingJump = null; // { el, at } -- what the in-flight jump aimed at

  function maxScrollY() {
    var doc = document.documentElement;
    return Math.max(0, (doc.scrollHeight || 0) - window.innerHeight);
  }

  // Instant, deliberately. Smooth scrolling on a held j queues one animation
  // per repeat and they fight each other; line-at-a-time motion has to track
  // the key exactly.
  function scrollLines(px) {
    window.scrollBy(0, px);
  }

  // Smooth, deliberately: a jump across the document is the one case where
  // the animation tells the reader which way they travelled.
  function scrollToY(y) {
    var target = Math.min(maxScrollY(), Math.max(0, y));
    try {
      window.scrollTo({ top: target, behavior: "smooth" });
    } catch (err) {
      window.scrollTo(0, target);
    }
  }

  function halfPage() {
    return Math.max(SCROLL_LINE, Math.round(window.innerHeight / 2));
  }

  function pageStep() {
    return Math.max(SCROLL_LINE, window.innerHeight - PAGE_OVERLAP);
  }

  function headingList(topLevelOnly) {
    var content = document.getElementById("mdview-content");
    if (!content) return [];
    var all = content.querySelectorAll("h1, h2, h3, h4, h5, h6");
    var out = [];
    for (var i = 0; i < all.length; i++) {
      var level = parseInt(all[i].tagName.substring(1), 10);
      if (!topLevelOnly || level <= 2) out.push(all[i]);
    }
    return out;
  }

  function jumpHeading(delta, topLevelOnly) {
    var list = headingList(topLevelOnly);
    if (!list.length) return;

    var target = null;
    var chained = -1;
    if (lastHeadingJump && Date.now() - lastHeadingJump.at < HEADING_CHAIN_MS) {
      chained = list.indexOf(lastHeadingJump.el);
    }
    if (chained >= 0) {
      target = list[chained + delta] || null;
    } else if (delta > 0) {
      for (var i = 0; i < list.length; i++) {
        if (list[i].getBoundingClientRect().top > HEADING_EPSILON) {
          target = list[i];
          break;
        }
      }
    } else {
      for (var j = list.length - 1; j >= 0; j--) {
        if (list[j].getBoundingClientRect().top < -HEADING_EPSILON) {
          target = list[j];
          break;
        }
      }
    }
    if (!target) return;
    lastHeadingJump = { el: target, at: Date.now() };
    // scrollY + rect.top is an absolute document offset, so it stays correct
    // even when read while a previous smooth scroll is still animating.
    scrollToY(window.scrollY + target.getBoundingClientRect().top - HEADING_MARGIN);
  }

  // n, N and enter step the search. With no live query they do nothing, rather
  // than opening the bar the way ⌘G does: enter in particular gets pressed for
  // all sorts of reasons and should not summon a search box.
  function stepFindKey(delta) {
    if (!findQuery) return;
    stepFind(delta);
  }

  function toggleSidebarKey() {
    var sidebar = document.getElementById("mdview-sidebar");
    if (sidebar) setSidebar(sidebar.hidden, sidebarTab);
  }

  // o and B name a tab rather than toggling blindly: pressing o while the
  // panel is showing bookmarks switches to the outline, and only a second
  // press -- when the outline is already what you are looking at -- closes it.
  function showSidebarTab(tab) {
    var sidebar = document.getElementById("mdview-sidebar");
    if (!sidebar) return;
    setSidebar(!(!sidebar.hidden && sidebarTab === tab), tab);
  }

  // The menu bar's View > Outline / Bookmarks land here. They SHOW a tab rather
  // than toggling it, unlike the o and b keys: picking "Outline" from a menu and
  // having the panel shut is not what anyone means by it.
  window.mdviewShowSidebarTab = function (tab) {
    setSidebar(true, tab === "bookmarks" ? "bookmarks" : "outline");
  };

  function cycleDiffLayout() {
    var root = document.documentElement;
    // Only meaningful in the diff view: a Markdown render has no unified and
    // split form to choose between.
    if (root.getAttribute("data-view") !== "diff") return;
    var next = root.getAttribute("data-diff-layout") === "split" ? "unified" : "split";
    postToHost("setDiffLayout:" + next);
  }

  // Opens whichever zoomable sits nearest the middle of the viewport, which is
  // the one being read. Without it the lightbox would be mouse-only to open.
  function zoomNearest() {
    var content = document.getElementById("mdview-content");
    if (!content) return;
    var nodes = content.querySelectorAll("[data-mdview-zoom]");
    var middle = window.innerHeight / 2;
    var best = null;
    var bestDistance = Infinity;
    for (var i = 0; i < nodes.length; i++) {
      var rect = nodes[i].getBoundingClientRect();
      // Skip anything with no box at all -- a diagram that failed to render.
      if (!rect.width && !rect.height) continue;
      var distance = Math.abs(rect.top + rect.height / 2 - middle);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = nodes[i];
      }
    }
    if (best) openLightbox(best);
  }

  function toggleDiffKey() {
    // Same condition the toolbar's own button is disabled under: a file with
    // no Git diff to show has nothing to toggle to. Leaving Diff is always
    // allowed, or an unavailable diff would be a one-way door.
    var inDiff = document.documentElement.getAttribute("data-view") === "diff";
    if (!inDiff && !window.mdviewDiffAvailable) return;
    postToHost("toggleDiff");
  }

  function toggleFullWidthKey() {
    // Outside the app there is no host to round-trip through, so flip the
    // attribute directly; the options menu follows it via its observer.
    if (postToHost("toggleFullWidth")) return;
    var root = document.documentElement;
    if (root.getAttribute("data-fullwidth") === "1") root.removeAttribute("data-fullwidth");
    else root.setAttribute("data-fullwidth", "1");
  }

  function reloadKey() {
    if (!postToHost("reloadDocument")) window.location.reload();
  }

  // Closing has to take the focus with it. A theme button inside a closed
  // <details> is still focused and still activates on space or Enter, so the
  // next keypress would commit a theme the user can no longer see -- the same
  // hazard mdviewCloseFind blurs its input for.
  // Arrow keys inside the open picker, mirroring what hover already does:
  // moving previews, only Enter (the button's own default) commits, and esc
  // reverts. Returns true when the key belonged to the picker.
  // `run: null` documents a key that something else already implements (the
  // find bar's own esc, the lightbox's keys): it reaches the cheat sheet but
  // never the dispatcher.
  var SHORTCUTS = [
    {
      title: "Moving",
      items: [
        { keys: ["j"], hint: "j", label: "Down a line", run: function () { scrollLines(SCROLL_LINE); } },
        { keys: ["k"], hint: "k", label: "Up a line", run: function () { scrollLines(-SCROLL_LINE); } },
        { keys: ["d"], hint: "d", label: "Half a page down", run: function () { scrollLines(halfPage()); } },
        { keys: ["u"], hint: "u", label: "Half a page up", run: function () { scrollLines(-halfPage()); } },
        { keys: [" "], hint: "space", label: "A page down", run: function () { scrollLines(pageStep()); } },
        { keys: ["Shift+Space"], hint: "⇧space", label: "A page up", run: function () { scrollLines(-pageStep()); } },
        { keys: [], hint: "g g", label: "Top of the document", run: null },
        { keys: ["G"], hint: "G", label: "Bottom of the document", run: function () { scrollToY(maxScrollY()); } },
      ],
    },
    {
      title: "Sections",
      items: [
        { keys: ["]"], hint: "]", label: "Next heading", run: function () { jumpHeading(1, false); } },
        { keys: ["["], hint: "[", label: "Previous heading", run: function () { jumpHeading(-1, false); } },
        { keys: ["}"], hint: "}", label: "Next top-level heading", run: function () { jumpHeading(1, true); } },
        { keys: ["{"], hint: "{", label: "Previous top-level heading", run: function () { jumpHeading(-1, true); } },
      ],
    },
    {
      title: "Finding",
      items: [
        { keys: ["/"], hint: "/", label: "Find in the document", run: function () { window.mdviewOpenFind(); } },
        { keys: [], hint: "enter", label: "Search, and back to the document", run: null },
        { keys: ["n", "Enter"], hint: "n  enter", label: "Next match", run: function () { stepFindKey(1); } },
        { keys: ["N"], hint: "N  ⇧enter", label: "Previous match", run: function () { stepFindKey(-1); } },
        { keys: [], hint: "esc", label: "Clear the search", run: null },
      ],
    },
    {
      title: "Sidebar",
      items: [
        { keys: ["s"], hint: "s", label: "Toggle the sidebar", run: toggleSidebarKey },
        { keys: ["o"], hint: "o", label: "Outline", run: function () { showSidebarTab("outline"); } },
        { keys: ["b"], hint: "b", label: "Bookmarks", run: function () { showSidebarTab("bookmarks"); } },
        { keys: ["m"], hint: "m", label: "Bookmark this document", run: function () { postToHost("toggleBookmark"); } },
        { keys: ["t"], hint: "t", label: "Themes", run: toggleThemePalette },
      ],
    },
    {
      title: "View",
      items: [
        { keys: ["D"], hint: "D", label: "Diff and back to Markdown", run: toggleDiffKey },
        { keys: ["l"], hint: "l", label: "Diff layout, unified or split", run: cycleDiffLayout },
        { keys: ["z"], hint: "z", label: "Zoom the nearest image", run: zoomNearest },
        { keys: ["w"], hint: "w", label: "Toggle full width", run: toggleFullWidthKey },
        { keys: ["r"], hint: "r", label: "Reload the document", run: reloadKey },
        { keys: ["+", "="], hint: "+", label: "Zoom in", run: function () { postToHost("zoomIn"); } },
        { keys: ["-"], hint: "−", label: "Zoom out", run: function () { postToHost("zoomOut"); } },
        { keys: ["0"], hint: "0", label: "Actual size", run: function () { postToHost("zoomReset"); } },
        { keys: ["?"], hint: "?", label: "This list", run: function () { toggleShortcuts(); } },
      ],
    },
    {
      title: "Themes",
      items: [
        { keys: [], hint: "type", label: "Filter the list", run: null },
        { keys: [], hint: "\u2191 \u2193", label: "Move, previewing as you go", run: null },
        { keys: [], hint: "enter", label: "Keep it", run: null },
        { keys: [], hint: "esc", label: "Put the old one back", run: null },
      ],
    },
    {
      title: "Zoomed image or diagram",
      items: [
        { keys: [], hint: "click  z", label: "Open it filling the window", run: null },
        { keys: [], hint: "+  −  0", label: "Zoom in, out, reset", run: null },
        { keys: [], hint: "↑ ↓ ← →", label: "Pan", run: null },
        { keys: [], hint: "esc", label: "Close", run: null },
      ],
    },
  ];

  var keyMap = null;

  function shortcutFor(key) {
    if (!keyMap) {
      keyMap = {};
      for (var g = 0; g < SHORTCUTS.length; g++) {
        var items = SHORTCUTS[g].items;
        for (var i = 0; i < items.length; i++) {
          if (!items[i].run) continue;
          for (var k = 0; k < items[i].keys.length; k++) {
            keyMap[items[i].keys[k]] = items[i];
          }
        }
      }
    }
    // hasOwnProperty, not a truth test: a key like "constructor" would
    // otherwise find something on Object.prototype and be "bound".
    return Object.prototype.hasOwnProperty.call(keyMap, key) ? keyMap[key] : null;
  }

  // ---- The ? cheat sheet ---------------------------------------------------
  //
  // Built from SHORTCUTS and appended to document.body, outside
  // #mdview-content, for the same reason the lightbox is: live reload replaces
  // that div's innerHTML and would destroy an overlay living inside it.

  function buildShortcutsOverlay() {
    var overlay = document.createElement("div");
    overlay.id = "mdview-shortcuts";
    overlay.hidden = true;
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-label", "Keyboard shortcuts");

    var panel = document.createElement("div");
    panel.className = "mdview-shortcuts-panel";

    var head = document.createElement("header");
    var heading = document.createElement("h2");
    heading.textContent = "Keyboard shortcuts";
    var close = document.createElement("button");
    close.type = "button";
    close.className = "mdview-shortcuts-close";
    close.setAttribute("aria-label", "Close");
    close.textContent = "×";
    head.appendChild(heading);
    head.appendChild(close);
    panel.appendChild(head);

    var groups = document.createElement("div");
    groups.className = "mdview-shortcuts-groups";
    for (var g = 0; g < SHORTCUTS.length; g++) {
      var section = document.createElement("section");
      var title = document.createElement("h3");
      title.textContent = SHORTCUTS[g].title;
      section.appendChild(title);
      var list = document.createElement("dl");
      for (var i = 0; i < SHORTCUTS[g].items.length; i++) {
        var entry = SHORTCUTS[g].items[i];
        var dt = document.createElement("dt");
        var parts = entry.hint.split("  ");
        for (var p = 0; p < parts.length; p++) {
          var kbd = document.createElement("kbd");
          kbd.textContent = parts[p];
          dt.appendChild(kbd);
        }
        var dd = document.createElement("dd");
        dd.textContent = entry.label;
        list.appendChild(dt);
        list.appendChild(dd);
      }
      section.appendChild(list);
      groups.appendChild(section);
    }
    panel.appendChild(groups);
    overlay.appendChild(panel);
    document.body.appendChild(overlay);

    overlay.addEventListener("click", function (event) {
      // Only the backdrop dismisses, not a click that bubbles out of the panel.
      if (event.target === overlay) closeShortcuts();
    });
    close.addEventListener("click", closeShortcuts);
    return overlay;
  }

  function shortcutsAreOpen() {
    var overlay = document.getElementById("mdview-shortcuts");
    return !!overlay && !overlay.hidden;
  }

  function closeShortcuts() {
    var overlay = document.getElementById("mdview-shortcuts");
    if (overlay) overlay.hidden = true;
  }

  function toggleShortcuts() {
    if (shortcutsAreOpen()) {
      closeShortcuts();
      return;
    }
    var overlay = document.getElementById("mdview-shortcuts") || buildShortcutsOverlay();
    overlay.hidden = false;
  }

  // The Help menu's item, and anything else on the native side, come through
  // here rather than synthesising a keypress.
  window.mdviewToggleShortcuts = function () {
    toggleShortcuts();
  };

  function isTextEntry(node) {
    if (!node || !node.tagName) return false;
    var tag = node.tagName.toLowerCase();
    if (tag === "input" || tag === "textarea" || tag === "select") return true;
    return !!node.isContentEditable;
  }

  // A focused button, link or disclosure activates on space and on enter.
  // Paging the document or stepping the search instead would make a theme in
  // the picker unreachable by keyboard.
  function activatesOnKey(node, key) {
    if (key !== " " && key !== "Enter") return false;
    if (!node || !node.tagName) return false;
    var tag = node.tagName.toLowerCase();
    return tag === "button" || tag === "a" || tag === "summary";
  }

  function onDocumentKeyDown(event) {
    // Any key at all means the hint has been read, or at least overtaken.
    // Above the modifier check on purpose, so ⌘-anything dismisses it too.
    dismissHint();
    // Modified keys belong to the menu bar (⌘F, ⌥⌘S, …) and to the browser.
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    // The lightbox is modal and owns the keyboard while it is up; its own
    // listener handles those keys.
    if (zoomState) return;

    if (shortcutsAreOpen()) {
      if (event.key === "Escape" || event.key === "?") {
        event.preventDefault();
        closeShortcuts();
      }
      // Nothing else reaches the document while the sheet is up.
      return;
    }

    // The palette is modal and owns the keyboard while it is up.
    if (themePaletteIsOpen()) {
      onThemePaletteKey(event);
      return;
    }

    // The find field and the palette's search box are the only places in this
    // page where a letter is a letter.
    if (isTextEntry(event.target) || isTextEntry(document.activeElement)) return;

    if (activatesOnKey(document.activeElement, event.key)) return;

    var key = event.key;
    // ⇧space pages back, the way it does in every pager, and ⇧enter steps the
    // search back. Both arrive under the unshifted key with shiftKey set, so
    // the shift is read here. "Shift+Space" is a name no event.key can take,
    // which is what keeps it clear of b -- the bookmarks key.
    if (key === " " && event.shiftKey) key = "Shift+Space";
    if (key === "Enter" && event.shiftKey) key = "N";

    if (key === "g") {
      event.preventDefault();
      if (pendingG && Date.now() - pendingG < G_CHORD_MS) {
        pendingG = 0;
        scrollToY(0);
      } else {
        pendingG = Date.now();
      }
      return;
    }
    // Any other key ends a half-typed chord: "gj" must not scroll to the top.
    pendingG = 0;

    var action = shortcutFor(key);
    if (!action) return;
    event.preventDefault();
    action.run();
  }

  function attachKeyListeners() {
    document.addEventListener("keydown", onDocumentKeyDown);
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
    if (!resizerEl) return;
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

  document.addEventListener("DOMContentLoaded", function () {
    attachSidebarListeners();
    attachFindListeners();
    attachKeyListeners();
    window.mdviewRenderAll();
  });
})();
