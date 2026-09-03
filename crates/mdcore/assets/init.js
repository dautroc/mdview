(function () {
  // Scoped, because re-rendering an already-rendered node feeds KaTeX its own
  // output as if it were TeX. The default root is the whole page, which is what
  // a fresh body swap wants; the rendered diff passes the block it just
  // hydrated.
  function renderMath(root) {
    if (typeof katex === "undefined") return;
    var nodes = (root || document).querySelectorAll(".math-inline, .math-display");
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
      // hjkl pan the same way, for hands already resting there.
      case "ArrowLeft":
      case "h":
        panBy(LIGHTBOX_PAN, 0);
        break;
      case "ArrowRight":
      case "l":
        panBy(-LIGHTBOX_PAN, 0);
        break;
      case "ArrowUp":
      case "k":
        panBy(0, LIGHTBOX_PAN);
        break;
      case "ArrowDown":
      case "j":
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
    // Painted again after the diagrams land: they arrive with a height, and
    // the first paint measured a document that did not have it yet.
    renderDiagrams().then(enhanceZoomables).then(scheduleMinimapPaint);
    renderSidebarBody();
    refreshHighlights();
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
          // The rendered diff's older versions. Out of the index even when a
          // reader has one open: find would offer matches in text that is not
          // in the document, and every comment anchored below it would shift.
          if (el.classList && el.classList.contains("mdview-rdiff-old")) {
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
    invalidateTextIndex();
  }

  function highlightFindMatches(query) {
    // Invalidates the text index on the way in, which also covers the early
    // return below.
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
    // Every split above moved the spans the cursor resolves through. The
    // offsets themselves are untouched -- the string is the same -- so this
    // only has to drop the node map.
    invalidateTextIndex();
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
    // Focusing the field collapses the selection in WebKit, so leave visual
    // mode deliberately rather than watching it be dismantled. exitVisual only
    // clears a selection this page painted, so the seed below still works for
    // one made with the mouse.
    exitVisual();
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
    layoutCommentRail();
    // The text column just changed width, so every block the map drew moved.
    scheduleMinimapPaint();
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
    if (title) {
      title.textContent =
        sidebarTab === "bookmarks" ? "Bookmarks" : sidebarTab === "comments" ? "Comments" : "Outline";
    }
    renderSidebarBody();
    layoutCommentRail();
    scheduleMinimapPaint();
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

  // ---- Transient acknowledgements -----------------------------------------
  //
  // A short line in the top-right for actions whose only other evidence is a
  // panel that may be shut. Top-RIGHT because the find bar owns the top-left
  // and the first-run hint owns the bottom; it steps aside when the sidebar
  // is open so it never lands on top of the list it is reporting about.

  var NOTE_LINGER_MS = 1800;
  var noteTimer = 0;

  function showNote(text) {
    var note = document.getElementById("mdview-note");
    if (!note) {
      note = document.createElement("div");
      note.id = "mdview-note";
      note.setAttribute("role", "status");
      document.body.appendChild(note);
    }
    note.textContent = text;
    // Toggling twice in a row rewrites the line in place and restarts the
    // countdown, so the second message is not cut short by the first's timer.
    clearTimeout(noteTimer);
    // Two frames, so a freshly inserted element is laid out before the
    // transition starts; adding the class in the same frame skips the fade.
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        note.classList.add("is-visible");
      });
    });
    noteTimer = setTimeout(function () {
      note.classList.remove("is-visible");
    }, NOTE_LINGER_MS);
  }

  // The host says some of the same things the page does -- "copied", "nothing
  // to copy" -- and they are news that expires, not a condition to resolve. A
  // banner would sit there until clicked; this is the same transient line the
  // page's own keys use, so one kind of message reads one way.
  window.mdviewNote = showNote;

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

  // The document's own headings. In the rendered diff an older version of a
  // block can be open on screen, and its headings are not the document's: they
  // would double the outline, land in ] and [, and index sections that are not
  // there.
  function documentHeadings(content) {
    var found = content.querySelectorAll("h1, h2, h3, h4, h5, h6");
    var out = [];
    for (var i = 0; i < found.length; i++) {
      if (found[i].closest && found[i].closest(".mdview-rdiff-old")) continue;
      out.push(found[i]);
    }
    return out;
  }

  // Build the outline from the rendered document. Rebuilt wholesale on every
  // render rather than patched: incremental updates over already-processed
  // nodes are the defect class that has bitten this project twice.
  function buildOutline() {
    var content = document.getElementById("mdview-content");
    if (!content) return "<p class=\"mdview-sidebar-empty\">No document.</p>";
    var headings = documentHeadings(content);
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
      // The list is rebuilt wholesale, so the mark saying where the reader is
      // standing has to be put back on the new rows every time.
      syncOutline();
    } else if (sidebarTab === "comments") {
      if (!comments.length) {
        body.innerHTML = "<p class=\"mdview-sidebar-empty\">No comments yet.</p>";
        return;
      }
      body.innerHTML = "<ul></ul>";
      var commentList = body.firstChild;
      for (var n = 0; n < comments.length; n++) {
        (function (comment) {
          var li = document.createElement("li");
          li.className = "mdview-comment-item";
          var a = document.createElement("a");
          a.href = "#";
          // textContent, never innerHTML: both fields are document text.
          // Elided on both: the row is a jump target in a column around 260px
          // wide, and a tooltip holding a whole section is not a tooltip.
          a.textContent = excerpt(comment.note || comment.quote, 120);
          a.title = excerpt(comment.quote, 300);
          if (commentOrphans[comment.id]) {
            li.classList.add("is-orphaned");
            a.title = "The text this quoted is gone:\n" + excerpt(comment.quote, 300);
          }
          a.addEventListener("click", function (event) {
            event.preventDefault();
            var marks = anchorMarks(comment.id);
            if (marks && marks.length) marks[0].scrollIntoView({ block: "center" });
          });
          li.appendChild(a);
          commentList.appendChild(li);
        })(comments[n]);
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

  // ---- The outline follows the reader --------------------------------------
  //
  // A map that does not say where you are standing is half a map. The section
  // you are in is the last heading above the line a heading jump lands on, so
  // ] and this agree about which heading you are "on" -- pressing ] lights the
  // row it took you to, rather than the one after it. Above the first heading
  // the first row is lit: a preamble reads as the document's opening, not as
  // nowhere.

  var outlineCurrentId = null;
  var outlineSyncPending = false;

  function currentHeadingId() {
    var content = contentEl();
    if (!content) return null;
    var headings = documentHeadings(content);
    if (!headings.length) return null;
    var found = null;
    for (var i = 0; i < headings.length; i++) {
      if (headings[i].getBoundingClientRect().top > HEADING_EPSILON) break;
      found = headings[i];
    }
    return (found || headings[0]).id || null;
  }

  // The sidebar's own scrollTop, never scrollIntoView: that one walks up to the
  // window and would scroll the document out from under the reader whose
  // scrolling is what called this in the first place.
  function revealOutlineRow(link, sidebar) {
    var pad = 24;
    var row = link.getBoundingClientRect();
    var box = sidebar.getBoundingClientRect();
    if (row.top < box.top + pad) sidebar.scrollTop += row.top - box.top - pad;
    else if (row.bottom > box.bottom - pad) sidebar.scrollTop += row.bottom - box.bottom + pad;
  }

  function syncOutline() {
    var sidebar = document.getElementById("mdview-sidebar");
    var body = document.getElementById("mdview-sidebar-body");
    if (!sidebar || !body || sidebar.hidden || sidebarTab !== "outline") {
      // Forgotten rather than remembered, so re-opening the panel scrolls the
      // row you are on back into view instead of assuming it stayed there.
      outlineCurrentId = null;
      return;
    }
    var id = currentHeadingId();
    var links = body.querySelectorAll("a[data-outline-id]");
    var current = null;
    for (var i = 0; i < links.length; i++) {
      var match = id !== null && links[i].getAttribute("data-outline-id") === id;
      links[i].classList.toggle("is-current", match);
      if (match) current = links[i];
    }
    // Only when it moves. A document long enough to need this has an outline
    // long enough to scroll, and nudging that list on every frame would fight
    // the reader who has just scrolled it by hand to look somewhere else.
    if (current && id !== outlineCurrentId) revealOutlineRow(current, sidebar);
    outlineCurrentId = id;
  }

  // The outline's mark and the minimap's viewport band answer the same
  // question, so they ride the same frame rather than one listener each.
  function schedulePositionSync() {
    if (outlineSyncPending) return;
    outlineSyncPending = true;
    requestAnimationFrame(function () {
      outlineSyncPending = false;
      followScrollWithCursor();
      syncOutline();
      placeMinimapWindow();
    });
  }

  window.mdviewSetSidebar = setSidebar;

  // ---- The minimap ---------------------------------------------------------
  //
  // A strip down the right edge holding the shape of the whole document: the
  // page seen from far enough away to take it all in at once, with the part
  // you are reading marked, and clickable.
  //
  // It is FIXED to the window rather than a column in the layout. Reserving
  // width was weighed for the comment rail and rejected there for a reason
  // that applies here too -- it re-wraps the text column, which moves your
  // place in the document. The rail is told about the strip instead, in
  // railGeometry, and the text keeps the width it had.
  //
  // What it draws is structure, not pixels. A scaled clone of the document
  // would duplicate every diagram, equation and image and would have to be
  // rebuilt on every save, and prose at this scale is a uniform grey anyway.
  // Headings are bars, prose is lines, code and tables are blocks, pictures
  // are frames: the shapes you actually navigate by.

  var MINIMAP_WIDTH = 72;
  // The strip's own padding, so nothing drawn touches either edge.
  var MINIMAP_PAD = 6;
  // Text lines are 1px on a 2px pitch. Below that they merge into a solid
  // block, and a solid block is what a code fence is supposed to look like.
  var MINIMAP_LINE_PITCH = 2;
  var minimapOpen = false;
  var minimapPaintPending = false;
  var minimapDrag = null;   // { moved, startY } while the button is down

  function minimapEl() {
    return document.getElementById("mdview-minimap");
  }

  // The diff view laid out as the document itself, rather than as its lines.
  // It has headings, a shape and a margin, so the things the source layouts
  // switch off stay on for it.
  function renderedDiff() {
    var root = document.documentElement;
    return root.getAttribute("data-view") === "diff" &&
      root.getAttribute("data-diff-layout") === "rendered";
  }

  // Hidden is not the only way to be absent: a source diff has no headings, no
  // comments and no margin, so the strip stays out of it entirely. The rendered
  // layout is the document, and gets it back.
  function minimapVisible() {
    var el = minimapEl();
    if (!el || el.hidden) return false;
    if (document.documentElement.getAttribute("data-view") !== "diff") return true;
    return renderedDiff();
  }

  // What the strip takes out of the window, and so out of the margin the
  // comment rail measures. Zero unless it is actually showing.
  function minimapReserve() {
    return minimapVisible() ? MINIMAP_WIDTH : 0;
  }

  // Empty when a theme leaves a token undefined -- find's colours are absent
  // in the System theme, where a <mark> keeps the browser's own yellow.
  function cssVar(style, name, fallback) {
    var value = style.getPropertyValue(name);
    value = value ? value.replace(/^\s+|\s+$/g, "") : "";
    return value || fallback;
  }

  // The whole document mapped onto the strip's height. A document shorter than
  // the window maps 1:1 and the viewport marker covers the lot, which is the
  // truth: there is nothing off screen to point at.
  function minimapScale() {
    var el = minimapEl();
    var docHeight = document.documentElement.scrollHeight || 1;
    var height = el && el.clientHeight ? el.clientHeight : window.innerHeight;
    return Math.min(1, height / docHeight);
  }

  function minimapKind(el) {
    var tag = el.tagName;
    if (/^H[1-6]$/.test(tag)) return "heading";
    if (tag === "HR") return "rule";
    // Before the PRE test, so a Mermaid diagram -- an <svg> inside a <pre> --
    // reads as the picture it became rather than as the source it was.
    if (tag === "IMG" || tag === "FIGURE") return "figure";
    if (el.querySelector && el.querySelector("img, svg")) return "figure";
    if (tag === "PRE" || tag === "TABLE") return "block";
    // A folded-away older version of a block, in the rendered diff.
    if (tag === "DETAILS") return "block";
    return "prose";
  }

  // The document's top-level blocks, in document coordinates. Rects rather
  // than offsetTop: the content div is not every block's offsetParent.
  function minimapBlocks() {
    var content = contentEl();
    if (!content) return [];
    var out = [];
    // The rendered diff wraps the document in one element, and the blocks the
    // map is about are its children rather than the content div's.
    var host = content.firstElementChild;
    if (!host || !host.classList || !host.classList.contains("mdview-rdiff")) host = content;
    var kids = host.children;
    for (var i = 0; i < kids.length; i++) {
      var el = kids[i];
      var rect = el.getBoundingClientRect();
      if (!rect.height) continue;
      var tag = el.tagName;
      out.push({
        top: rect.top + window.scrollY,
        height: rect.height,
        kind: minimapKind(el),
        level: /^H[1-6]$/.test(tag) ? parseInt(tag.charAt(1), 10) : 0,
      });
    }
    return out;
  }

  // Prose as the lines it is made of, while there is room for lines. The last
  // line of a paragraph is short: it is the only thing that makes a stack of
  // lines read as prose rather than as a filled box.
  function paintMinimapProse(ctx, colour, y, h, width) {
    ctx.globalAlpha = 0.38;
    ctx.fillStyle = colour;
    if (h < MINIMAP_LINE_PITCH * 2) {
      ctx.fillRect(0, y, width, Math.max(1, h));
      return;
    }
    var lines = Math.floor(h / MINIMAP_LINE_PITCH);
    for (var i = 0; i < lines; i++) {
      ctx.fillRect(0, y + i * MINIMAP_LINE_PITCH, i === lines - 1 ? width * 0.6 : width, 1);
    }
  }

  // Comments down the right edge, find matches down the left, so a passage
  // that is both does not hide one mark under the other.
  function paintMinimapMarkers(ctx, style, scale, width) {
    var anchors = commentAnchors || [];
    var matches = findMatches || [];
    ctx.globalAlpha = 1;
    ctx.fillStyle = cssVar(style, "--comment-fg", "#4b2c85");
    for (var a = 0; a < anchors.length; a++) {
      var marks = anchors[a].marks;
      if (!marks || !marks.length) continue;
      ctx.fillRect(width - 3, minimapMarkTop(marks[0]) * scale - 1, 3, 3);
    }
    ctx.fillStyle = cssVar(style, "--find-hit-bg",
      cssVar(style, "--find-current-bg", cssVar(style, "--link", "#0969da")));
    for (var f = 0; f < matches.length; f++) {
      ctx.fillRect(0, minimapMarkTop(matches[f]) * scale - 1, 3, 3);
    }
  }

  function minimapMarkTop(el) {
    return el.getBoundingClientRect().top + window.scrollY;
  }

  function paintMinimap() {
    var el = minimapEl();
    var canvas = document.getElementById("mdview-minimap-canvas");
    if (!el || !canvas || !minimapVisible()) return;
    var width = el.clientWidth - MINIMAP_PAD * 2;
    var height = el.clientHeight;
    if (width <= 0 || height <= 0) return;
    var ctx = canvas.getContext ? canvas.getContext("2d") : null;
    if (!ctx) return;
    // The backing store is in device pixels and the box is in CSS pixels, or
    // every line drawn here would be a blurred two.
    var ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    canvas.style.width = width + "px";
    canvas.style.height = height + "px";
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);

    var style = getComputedStyle(document.documentElement);
    var fg = cssVar(style, "--fg", "#1f2328");
    var muted = cssVar(style, "--muted", "#59636e");
    var border = cssVar(style, "--border", "#d1d9e0");
    var scale = minimapScale();
    var blocks = minimapBlocks();

    for (var i = 0; i < blocks.length; i++) {
      var b = blocks[i];
      var y = b.top * scale;
      var h = Math.max(1, b.height * scale);
      if (b.kind === "heading") {
        // Deeper headings are shorter bars, so the shape of the document shows
        // its hierarchy the way the outline's indentation does.
        ctx.globalAlpha = 1;
        ctx.fillStyle = fg;
        ctx.fillRect(0, y, Math.max(8, width * (1 - (b.level - 1) * 0.13)), Math.max(2, Math.min(h, 3)));
      } else if (b.kind === "block") {
        ctx.globalAlpha = 0.5;
        ctx.fillStyle = muted;
        ctx.fillRect(0, y, width, h);
      } else if (b.kind === "figure") {
        // Half-pixel offsets, or a 1px stroke straddles two rows and greys.
        ctx.globalAlpha = 0.8;
        ctx.strokeStyle = border;
        ctx.lineWidth = 1;
        ctx.strokeRect(0.5, Math.round(y) + 0.5, width - 1, Math.max(2, Math.round(h) - 1));
      } else if (b.kind === "rule") {
        ctx.globalAlpha = 0.8;
        ctx.fillStyle = border;
        ctx.fillRect(0, y, width, 1);
      } else {
        paintMinimapProse(ctx, muted, y, h, width);
      }
    }
    ctx.globalAlpha = 1;
    paintMinimapMarkers(ctx, style, scale, width);
  }

  // The band behind the map, not a box drawn over it: it sits under the canvas
  // exactly as the palette's current row sits under its text, which is where
  // its colour comes from.
  function placeMinimapWindow() {
    var win = document.getElementById("mdview-minimap-window");
    if (!win || !minimapVisible()) return;
    var scale = minimapScale();
    win.style.top = Math.round(window.scrollY * scale) + "px";
    win.style.height = Math.max(6, Math.round(window.innerHeight * scale)) + "px";
  }

  function scheduleMinimapPaint() {
    if (minimapPaintPending) return;
    minimapPaintPending = true;
    requestAnimationFrame(function () {
      minimapPaintPending = false;
      paintMinimap();
      placeMinimapWindow();
    });
  }

  // Where in the document a point on the strip is, with the viewport centred
  // on it: you click what you want to read, not the top edge of it.
  function minimapTarget(clientY) {
    var el = minimapEl();
    var scale = minimapScale();
    if (!el || !scale) return 0;
    return (clientY - el.getBoundingClientRect().top) / scale - window.innerHeight / 2;
  }

  function onMinimapMouseDown(event) {
    if (!minimapVisible()) return;
    if (event.button != null && event.button !== 0) return;
    event.preventDefault();
    minimapDrag = { moved: false, startY: event.clientY };
    document.addEventListener("mousemove", onMinimapMouseMove);
    document.addEventListener("mouseup", onMinimapMouseUp);
  }

  function onMinimapMouseMove(event) {
    if (!minimapDrag) return;
    if (!minimapDrag.moved && Math.abs(event.clientY - minimapDrag.startY) < 3) return;
    minimapDrag.moved = true;
    // Instant while the button is down, for the reason scrollLines is: one
    // smooth animation per move event would queue up and fight the pointer.
    window.scrollTo(0, Math.min(maxScrollY(), Math.max(0, minimapTarget(event.clientY))));
  }

  function onMinimapMouseUp(event) {
    if (!minimapDrag) return;
    var moved = minimapDrag.moved;
    minimapDrag = null;
    document.removeEventListener("mousemove", onMinimapMouseMove);
    document.removeEventListener("mouseup", onMinimapMouseUp);
    // A press that never became a drag is a jump across the document, which is
    // the one case the animation is worth having.
    if (!moved) scrollToY(minimapTarget(event.clientY));
  }

  function setMinimap(open) {
    var el = minimapEl();
    if (!el) return;
    minimapOpen = !!open;
    el.hidden = !minimapOpen;
    document.documentElement.setAttribute("data-minimap-open", minimapOpen ? "1" : "0");
    document.documentElement.style.setProperty("--minimap-width", MINIMAP_WIDTH + "px");
    // The rail has just been handed back, or charged, the width of the strip.
    layoutCommentRail();
    scheduleMinimapPaint();
    postToHost("setMinimap:" + (minimapOpen ? "1" : "0"));
  }

  function toggleMinimapKey() {
    setMinimap(!minimapOpen);
  }

  window.mdviewSetMinimap = function (open) {
    setMinimap(!!open);
  };


  // ---- Document options ---------------------------------------------------
  window.mdviewDiffAvailable = false;
  // Why not, in the host's words, so D can say something better than nothing.
  // Null until the host has spoken, which is also while nothing is available.
  var diffUnavailableReason = null;
  window.mdviewSetDiffAvailability = function (available, reason) {
    window.mdviewDiffAvailable = !!available;
    diffUnavailableReason = reason || null;
  };
  window.mdviewSetViewState = function (view, layout, fullWidth, available, reason) {
    var root = document.documentElement;
    if (view === "diff") root.setAttribute("data-view", "diff");
    else root.removeAttribute("data-view");
    if (layout) root.setAttribute("data-diff-layout", layout);
    if (fullWidth) root.setAttribute("data-fullwidth", "1");
    else root.removeAttribute("data-fullwidth");
    if (typeof available === "boolean") window.mdviewDiffAvailable = available;
    if (arguments.length > 4) diffUnavailableReason = reason || null;
    // The diff has no shape to map and full width leaves no margin to sit in.
    scheduleMinimapPaint();
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
    exitVisual();
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

  // ---- Recent files palette -------------------------------------------------
  //
  // The same shell as the theme palette, and there for the reason File > Open
  // Recent is not enough: the history is fifty documents long, a native menu
  // shows it one item at a time to a mouse, and the thing you actually know
  // about the file you want is a few letters of its name.
  //
  // Its rows, unlike the theme list's, are rebuilt every time it opens. The
  // themes are fixed at build time; the history changes on every open, and the
  // host pushes a new list each time.

  var recents = [];         // [{ name, dir, path }], newest first
  var recentRows = [];      // every row, in list order
  var recentMatches = [];   // the rows currently passing the filter
  var recentIndex = -1;     // which of recentMatches is highlighted

  // The host sends each window the list with its own document taken out, so
  // there is nothing to filter here.
  window.mdviewSetRecents = function (items) {
    recents = items || [];
    // A push while the palette is up (another window opened something) has to
    // reach the list, or it would be showing history that has moved on.
    if (recentPaletteIsOpen()) {
      var input = recentPaletteInput();
      renderRecentRows();
      filterRecents(input ? input.value : "");
    }
  };

  function recentPaletteEl() {
    return document.getElementById("mdview-recent-palette");
  }

  function recentPaletteInput() {
    return document.getElementById("mdview-recent-search");
  }

  function recentPaletteIsOpen() {
    var el = recentPaletteEl();
    return !!el && !el.hidden;
  }

  function buildRecentPalette() {
    var overlay = document.createElement("div");
    overlay.id = "mdview-recent-palette";
    overlay.hidden = true;
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-label", "Recent files");

    var panel = document.createElement("div");
    panel.className = "mdview-palette-panel";

    var input = document.createElement("input");
    input.type = "text";
    input.id = "mdview-recent-search";
    input.className = "mdview-palette-search";
    input.placeholder = "Recent files";
    input.setAttribute("aria-label", "Search recent files");
    input.setAttribute("autocomplete", "off");
    input.setAttribute("autocorrect", "off");
    input.setAttribute("spellcheck", "false");
    panel.appendChild(input);

    var list = document.createElement("div");
    list.className = "mdview-palette-list";
    list.id = "mdview-recent-list";
    list.setAttribute("role", "listbox");
    panel.appendChild(list);

    var empty = document.createElement("p");
    empty.className = "mdview-palette-empty";
    empty.id = "mdview-recent-empty";
    empty.hidden = true;
    panel.appendChild(empty);

    overlay.appendChild(panel);
    document.body.appendChild(overlay);

    overlay.addEventListener("click", function (event) {
      // Only the backdrop dismisses, not a click bubbling out of the panel.
      if (event.target === overlay) closeRecentPalette();
    });
    input.addEventListener("input", function () {
      filterRecents(input.value);
    });
    return overlay;
  }

  function renderRecentRows() {
    var list = document.getElementById("mdview-recent-list");
    if (!list) return;
    list.innerHTML = "";
    recentRows = [];
    recentMatches = [];
    recentIndex = -1;
    for (var i = 0; i < recents.length; i++) {
      (function (entry) {
        var row = document.createElement("button");
        row.type = "button";
        row.className = "mdview-palette-row";
        row.setAttribute("role", "option");
        row.setAttribute("data-path", entry.path);
        row.title = entry.path;

        var name = document.createElement("span");
        name.className = "mdview-palette-name";
        name.textContent = entry.name;   // textContent, never innerHTML
        row.appendChild(name);
        // Two documents called README.md are only tellable apart by where they
        // live, so the folder is part of the row rather than only the tooltip
        // -- and, being in the row, it is part of what the filter reads.
        if (entry.dir) {
          var dir = document.createElement("span");
          dir.className = "mdview-palette-dir";
          dir.textContent = entry.dir;
          row.appendChild(dir);
        }

        row.addEventListener("mouseenter", function () {
          highlightRecent(recentMatches.indexOf(row));
        });
        row.addEventListener("click", function () {
          openRecent(row);
        });
        list.appendChild(row);
        recentRows.push(row);
      })(recents[i]);
    }
  }

  // No preview here, unlike the theme palette: moving the highlight cannot
  // show you a document without opening one, and opening one is the commit.
  function highlightRecent(index) {
    if (!recentMatches.length) {
      recentIndex = -1;
      return;
    }
    var count = recentMatches.length;
    recentIndex = ((index % count) + count) % count;
    for (var i = 0; i < recentRows.length; i++) {
      recentRows[i].classList.remove("is-current");
      recentRows[i].setAttribute("aria-selected", "false");
    }
    var row = recentMatches[recentIndex];
    row.classList.add("is-current");
    row.setAttribute("aria-selected", "true");
    if (row.scrollIntoView) row.scrollIntoView({ block: "nearest" });
  }

  function filterRecents(query) {
    var needle = (query || "").toLowerCase().trim();
    recentMatches = [];
    for (var i = 0; i < recentRows.length; i++) {
      var hit = !needle || recentRows[i].textContent.toLowerCase().indexOf(needle) >= 0;
      recentRows[i].hidden = !hit;
      if (hit) recentMatches.push(recentRows[i]);
    }
    var empty = document.getElementById("mdview-recent-empty");
    if (empty) {
      // An empty history and a query that matches nothing are different
      // states, and the second one is the only one you can do anything about.
      empty.textContent = recentRows.length
        ? "No recent files match."
        : "Nothing else has been opened yet.";
      empty.hidden = recentMatches.length > 0;
    }
    // A narrowed list starts at its first match, the way the theme palette's
    // does: the highlight must never sit on a row no longer on screen.
    if (recentMatches.length) highlightRecent(0);
    else recentIndex = -1;
  }

  function openRecent(row) {
    if (!row) return;
    var path = row.getAttribute("data-path");
    if (!path) return;
    closeRecentPalette();
    postToHost("openPath:" + path);
  }

  function closeRecentPalette() {
    var overlay = recentPaletteEl();
    if (!overlay) return;
    overlay.hidden = true;
    var input = recentPaletteInput();
    if (input) input.blur();
  }

  function openRecentPalette() {
    exitVisual();
    var overlay = recentPaletteEl() || buildRecentPalette();
    overlay.hidden = false;
    renderRecentRows();
    var input = recentPaletteInput();
    if (input) {
      input.value = "";
      input.focus();
    }
    // Opens on the most recently opened OTHER document, so g r enter is "back
    // to the one before this" without reading the list at all.
    filterRecents("");
  }

  function toggleRecentPalette() {
    if (recentPaletteIsOpen()) closeRecentPalette();
    else openRecentPalette();
  }

  // Driven from the document handler rather than from the search field, for
  // the reason the theme palette's is: a row clicked with the mouse takes the
  // focus off the input, and the arrows have to keep steering the list.
  function onRecentPaletteKey(event) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        closeRecentPalette();
        break;
      case "Enter":
        event.preventDefault();
        openRecent(recentMatches[recentIndex]);
        break;
      case "ArrowDown":
        event.preventDefault();
        highlightRecent(recentIndex + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        highlightRecent(recentIndex - 1);
        break;
      default:
        break;
    }
  }


  // ---- Command palette ------------------------------------------------------
  //
  // The third palette on the same shell, and the only one with no list of its
  // own: its rows ARE the SHORTCUTS table, read at open time. A command is in
  // the palette because it is documented, so the two can never disagree, and
  // the key it names is the one the dispatcher would have run.
  //
  // It is the answer to the one thing a single-key app cannot do: be searched.
  // The ? sheet is the map you read top to bottom; this is for when you know
  // what you want to DO and not which key does it -- and, having found it, you
  // leave with the key.

  var commandRows = [];     // every row, in table order
  var commandEntries = [];  // the SHORTCUTS item each row runs, same order
  var commandMatches = [];  // the rows currently passing the filter
  var commandIndex = -1;    // which of commandMatches is highlighted

  function commandPaletteEl() {
    return document.getElementById("mdview-command-palette");
  }

  function commandPaletteInput() {
    return document.getElementById("mdview-command-search");
  }

  function commandPaletteIsOpen() {
    var el = commandPaletteEl();
    return !!el && !el.hidden;
  }

  function buildCommandPalette() {
    var overlay = document.createElement("div");
    overlay.id = "mdview-command-palette";
    overlay.hidden = true;
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-label", "Commands");

    var panel = document.createElement("div");
    panel.className = "mdview-palette-panel";

    var input = document.createElement("input");
    input.type = "text";
    input.id = "mdview-command-search";
    input.className = "mdview-palette-search";
    input.placeholder = "Run a command";
    input.setAttribute("aria-label", "Search commands");
    input.setAttribute("autocomplete", "off");
    input.setAttribute("autocorrect", "off");
    input.setAttribute("spellcheck", "false");
    panel.appendChild(input);

    var list = document.createElement("div");
    list.className = "mdview-palette-list";
    list.id = "mdview-command-list";
    list.setAttribute("role", "listbox");
    panel.appendChild(list);

    var empty = document.createElement("p");
    empty.className = "mdview-palette-empty";
    empty.id = "mdview-command-empty";
    empty.hidden = true;
    empty.textContent = "No command matches.";
    panel.appendChild(empty);

    overlay.appendChild(panel);
    document.body.appendChild(overlay);

    overlay.addEventListener("click", function (event) {
      // Only the backdrop dismisses, not a click bubbling out of the panel.
      if (event.target === overlay) closeCommandPalette();
    });
    input.addEventListener("input", function () {
      filterCommands(input.value);
    });
    return overlay;
  }

  function renderCommandRows() {
    var list = document.getElementById("mdview-command-list");
    if (!list) return;
    list.innerHTML = "";
    commandRows = [];
    commandEntries = [];
    commandMatches = [];
    commandIndex = -1;
    for (var g = 0; g < SHORTCUTS.length; g++) {
      var group = SHORTCUTS[g];
      for (var i = 0; i < group.items.length; i++) {
        var entry = group.items[i];
        // Three kinds of row never reach the palette. One documenting a key
        // something else implements has nothing to run; the palette's own row
        // would only reopen what you are looking at -- the argument that keeps
        // the open document out of the recent files list; and a key that is
        // VIM'S rather than MDView's is not a thing you came here to look up.
        // Nobody searches a palette for j, and printing the whole vim alphabet
        // buries the dozen commands that are this app's own.
        if (!entry.run || entry.vim || entry.run === toggleCommandPalette) continue;
        list.appendChild(commandRow(entry, group.title));
      }
    }
  }

  function commandRow(entry, groupTitle) {
    var row = document.createElement("button");
    row.type = "button";
    row.className = "mdview-palette-row mdview-command-row";
    row.setAttribute("role", "option");
    row.title = entry.label;

    var label = document.createElement("span");
    label.className = "mdview-palette-name";
    label.textContent = entry.label;   // textContent, never innerHTML
    row.appendChild(label);

    var keys = document.createElement("span");
    keys.className = "mdview-command-keys";
    // The hint is split the way the ? sheet splits it, so a chord arrives as
    // two keycaps and reads as two presses rather than one impossible key.
    var parts = entry.hint.split("  ");
    for (var p = 0; p < parts.length; p++) {
      var cap = document.createElement("kbd");
      cap.textContent = parts[p];
      keys.appendChild(cap);
    }
    row.appendChild(keys);

    // The group title is searched but not shown: "scroll" should find the
    // scrolling commands, and printing "Scrolling" on four rows that are
    // already next to each other would spend the width the labels need.
    row.setAttribute("data-search", (entry.label + " " + groupTitle + " " + entry.hint).toLowerCase());
    row.addEventListener("mouseenter", function () {
      highlightCommand(commandMatches.indexOf(row));
    });
    row.addEventListener("click", function () {
      runCommand(row);
    });
    commandRows.push(row);
    commandEntries.push(entry);
    return row;
  }

  // No preview, like the recent files list and unlike the themes: an action is
  // only visible once it has run, and running it is the commit.
  function highlightCommand(index) {
    if (!commandMatches.length) {
      commandIndex = -1;
      return;
    }
    var count = commandMatches.length;
    commandIndex = ((index % count) + count) % count;
    for (var i = 0; i < commandRows.length; i++) {
      commandRows[i].classList.remove("is-current");
      commandRows[i].setAttribute("aria-selected", "false");
    }
    var row = commandMatches[commandIndex];
    row.classList.add("is-current");
    row.setAttribute("aria-selected", "true");
    if (row.scrollIntoView) row.scrollIntoView({ block: "nearest" });
  }

  function filterCommands(query) {
    var needle = (query || "").toLowerCase().trim();
    commandMatches = [];
    for (var i = 0; i < commandRows.length; i++) {
      var hay = commandRows[i].getAttribute("data-search") || "";
      var hit = !needle || hay.indexOf(needle) >= 0;
      commandRows[i].hidden = !hit;
      if (hit) commandMatches.push(commandRows[i]);
    }
    var empty = document.getElementById("mdview-command-empty");
    if (empty) empty.hidden = commandMatches.length > 0;
    // A narrowed list starts at its first match: the highlight must never sit
    // on a row no longer on screen.
    if (commandMatches.length) highlightCommand(0);
    else commandIndex = -1;
  }

  // Closed FIRST, which is also what puts the selection back: a command acts
  // on the document, and several act on the selection, so it has to be there
  // before the command runs rather than after.
  function runCommand(row) {
    if (!row) return;
    var at = commandRows.indexOf(row);
    var entry = at < 0 ? null : commandEntries[at];
    if (!entry) return;
    closeCommandPalette();
    entry.run();
  }

  // The search field destroyed the visual selection by taking the focus -- a
  // window has exactly one Selection -- but only the painted one: the model
  // behind it survived, so painting it again restores it exactly. Done on
  // close rather than only on run, or an esc out of the palette would leave
  // visual mode on with nothing highlighted.
  function closeCommandPalette() {
    var overlay = commandPaletteEl();
    if (!overlay) return;
    // Blurred BEFORE the overlay is hidden, and this order is load-bearing:
    // hiding the field first leaves it the focused element until WebKit gets
    // round to resetting focus, and a command that runs in between reads a
    // focus that has gone. `y` from the palette copied nothing, because the
    // copy went to an empty field rather than to the document.
    var input = commandPaletteInput();
    if (input) input.blur();
    overlay.hidden = true;
    paintVisual();
  }

  // Deliberately does NOT exitVisual, unlike the other two palettes: `v` then
  // `:` then "Copy" is a sentence, and leaving visual mode would delete its
  // subject halfway through.
  function openCommandPalette() {
    var overlay = commandPaletteEl() || buildCommandPalette();
    overlay.hidden = false;
    renderCommandRows();
    var input = commandPaletteInput();
    if (input) {
      input.value = "";
      input.focus();
    }
    filterCommands("");
  }

  function toggleCommandPalette() {
    if (commandPaletteIsOpen()) closeCommandPalette();
    else openCommandPalette();
  }

  // Driven from the document handler rather than from the search field, for
  // the reason the other two are: a row clicked with the mouse takes the focus
  // off the input, and the arrows have to keep steering the list.
  function onCommandPaletteKey(event) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        closeCommandPalette();
        break;
      case "Enter":
        event.preventDefault();
        runCommand(commandMatches[commandIndex]);
        break;
      case "ArrowDown":
        event.preventDefault();
        highlightCommand(commandIndex + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        highlightCommand(commandIndex - 1);
        break;
      default:
        break;
    }
  }

  // ---- Comments -------------------------------------------------------------
  //
  // A comment is anchored by (heading ordinal, quote, nth occurrence), never by
  // a heading id: buildOutline is the only code that assigns those, and it runs
  // only while the outline panel is showing, so an id is a field that is
  // frequently absent.
  //
  // Anchors are re-applied from scratch after every render, like the outline
  // and unlike anything incremental -- patching already-processed nodes is the
  // defect class that has bitten this project twice.

  var comments = [];
  var commentAnchors = [];
  var commentOrphans = {};
  var pendingComment = null;
  var editingCommentId = null;
  // A sanity bound, not an anchoring constraint. indexOf does not care how
  // long the quote is and fence_for wraps a payload of any size, so the old
  // 400 refused selections the machinery would have anchored fine. What a long
  // quote actually costs is the overlap rule in applyCommentAnchors -- it
  // claims a span no finer comment can then sit inside -- and that is a
  // judgement the person selecting is entitled to make. The cap is now only
  // where a selection stops being a passage and starts being the document.
  var COMMENT_QUOTE_MAX = 4000;

  // The one place a quote is cut down for display. Three surfaces show one --
  // the draft label, the sidebar row, the note strip -- and each used to decide
  // for itself, or not at all, which is what made the cap load-bearing for
  // layout as well as for anchoring.
  //
  // Whitespace is flattened first so the budget is spent on words rather than
  // on a code block's indentation, and so a quote spanning several lines
  // cannot turn a one-line row or a tooltip into a wall.
  function excerpt(text, max) {
    if (!text) return "";
    var flat = text.replace(/\s+/g, " ").replace(/^\s+|\s+$/g, "");
    return flat.length > max ? flat.slice(0, max) + "…" : flat;
  }

  function contentEl() {
    return document.getElementById("mdview-content");
  }

  // The concatenated plain text of a root, plus the map back to the text nodes
  // it came from.
  //
  // This exists because Selection.toString() concatenates across inline
  // elements: selecting `the **bold** word` yields "the bold word", a string
  // that appears in no single text node. Find matches one node at a time, which
  // is why it silently misses phrases containing code, bold or a link; reusing
  // that here would orphan most real selections on the first save.
  function textIndex(root) {
    var nodes = collectFindTextNodes(root);
    var text = "";
    var spans = [];
    for (var i = 0; i < nodes.length; i++) {
      var value = nodes[i].nodeValue;
      spans.push({ node: nodes[i], start: text.length, end: text.length + value.length });
      text += value;
    }
    return { text: text, spans: spans };
  }

  // Map a [from, to) slice of that concatenated text back to one run per text
  // node it crosses.
  function runsFor(index, from, to) {
    var runs = [];
    for (var i = 0; i < index.spans.length; i++) {
      var span = index.spans[i];
      if (span.end <= from) continue;
      if (span.start >= to) break;
      runs.push({
        node: span.node,
        start: Math.max(from, span.start) - span.start,
        end: Math.min(to, span.end) - span.start,
      });
    }
    return runs;
  }

  // Wrap each run in its own <mark>. Deliberately not Range.surroundContents,
  // which throws on a partially selected node -- precisely this case. One
  // comment therefore becomes several marks sharing a data-comment-id, which
  // read as a single highlight because the class carries no padding.
  function wrapRuns(runs, id) {
    var marks = [];
    for (var i = 0; i < runs.length; i++) {
      var run = runs[i];
      if (run.end <= run.start) continue;
      var hit = run.start > 0 ? run.node.splitText(run.start) : run.node;
      if (hit.nodeValue.length > run.end - run.start) hit.splitText(run.end - run.start);
      var mark = document.createElement("mark");
      mark.className = "mdview-comment-anchor";
      mark.setAttribute("data-comment-id", id);
      hit.parentNode.replaceChild(mark, hit);
      mark.appendChild(hit);
      marks.push(mark);
    }
    invalidateTextIndex();
    return marks;
  }

  // Where each heading's section starts in the concatenated text: the offset of
  // the first text node at or after it.
  function sectionStarts(index) {
    var content = contentEl();
    if (!content) return [];
    var headings = documentHeadings(content);
    var starts = new Array(headings.length);
    var k = 0;
    for (var i = 0; i < index.spans.length && k < headings.length; i++) {
      var node = index.spans[i].node;
      while (k < headings.length && headingReaches(headings[k], node)) {
        starts[k] = index.spans[i].start;
        k++;
      }
    }
    for (; k < headings.length; k++) starts[k] = index.text.length;
    return starts;
  }

  function headingReaches(heading, node) {
    if (heading.contains(node)) return true;
    return (heading.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
  }

  // The [from, to) slice a comment's heading ordinal names. 0 is the text above
  // the first heading.
  function sectionRange(index, starts, heading) {
    if (!starts.length) return { from: 0, to: index.text.length };
    if (heading <= 0) return { from: 0, to: starts.length ? starts[0] : index.text.length };
    var from = heading - 1 < starts.length ? starts[heading - 1] : index.text.length;
    var to = heading < starts.length ? starts[heading] : index.text.length;
    return { from: from, to: to };
  }

  function clearCommentAnchors() {
    for (var i = 0; i < commentAnchors.length; i++) {
      var marks = commentAnchors[i].marks;
      for (var j = 0; j < marks.length; j++) {
        var mark = marks[j];
        var parent = mark.parentNode;
        if (!parent) continue;
        parent.replaceChild(document.createTextNode(mark.textContent), mark);
        parent.normalize();
      }
    }
    commentAnchors = [];
    invalidateTextIndex();
  }

  // Re-find every comment and highlight it.
  //
  // Ranges are resolved against ONE pristine index, then settled between where
  // they overlap, and only then wrapped -- three passes with two different
  // orderings, which is why they are separate loops. See each one.
  function applyCommentAnchors() {
    clearCommentAnchors();
    commentOrphans = {};
    var content = contentEl();
    if (!content || !comments.length) return;
    var index = textIndex(content);
    var starts = sectionStarts(index);
    var found = [];
    for (var i = 0; i < comments.length; i++) {
      var comment = comments[i];
      if (!comment.quote) continue;
      var range = sectionRange(index, starts, comment.heading);
      var section = index.text.slice(range.from, range.to);
      var at = -1;
      var seen = -1;
      var cursor = 0;
      while (cursor <= section.length) {
        var hit = section.indexOf(comment.quote, cursor);
        if (hit < 0) break;
        seen++;
        at = hit;
        if (seen >= comment.nth) break;
        cursor = hit + 1;
      }
      if (at < 0) {
        // The text it quoted is gone. Kept in the list, marked orphaned: a
        // comment whose subject was edited away is still something you wrote.
        commentOrphans[comment.id] = true;
        continue;
      }
      found.push({
        comment: comment,
        order: i,
        from: range.from + at,
        to: range.from + at + comment.quote.length,
      });
    }
    // Which overlapping anchor gets the highlight. Nested marks would destroy
    // each other on the next clear -- clearCommentAnchors replaces a mark with
    // a flat text node, taking anything inside it with it -- so of two comments
    // whose spans overlap, exactly one can be drawn and the other shows in the
    // list without a highlight.
    //
    // The ENCLOSING one wins. A comment on a whole passage and a comment on
    // three words inside it are not rivals: the wide one is the reason the
    // narrow one has a context at all, and letting the narrow one win strikes
    // through the comment about the section on the strength of an aside. Widest
    // first is what makes that fall out, because an enclosing span is strictly
    // longer than anything it contains. Partial overlaps, where neither
    // encloses the other, go the same way -- the wider span is the one more of
    // the document loses with it.
    //
    // Ties: two spans of one width by position, and two comments on the very
    // same words by the order they arrived in, so the winner is never left to
    // sort stability.
    var byWidth = found.slice().sort(function (a, b) {
      return b.to - b.from - (a.to - a.from) || a.from - b.from || a.order - b.order;
    });
    var claimed = [];
    for (var f = 0; f < byWidth.length; f++) {
      var entry = byWidth[f];
      var overlaps = false;
      for (var c = 0; c < claimed.length; c++) {
        if (entry.from < claimed[c].to && claimed[c].from < entry.to) overlaps = true;
      }
      if (overlaps) {
        commentOrphans[entry.comment.id] = true;
        continue;
      }
      claimed.push(entry);
    }
    // Back to front, which is a different order for a different reason and not
    // the one above: wrapRuns splits the very text nodes this index was built
    // from, so wrapping a span invalidates the runs of everything after it.
    // Descending, a split can only ever touch offsets already spent.
    claimed.sort(function (a, b) {
      return b.from - a.from;
    });
    for (var w = 0; w < claimed.length; w++) {
      var winner = claimed[w];
      var marks = wrapRuns(runsFor(index, winner.from, winner.to), winner.comment.id);
      if (marks.length) commentAnchors.push({ id: winner.comment.id, marks: marks });
      else commentOrphans[winner.comment.id] = true;
    }
  }

  // The one funnel for both highlight layers, and the reason it exists: find
  // tears its marks down by replacing them with a flat text node, which DELETES
  // anything nested inside. So comment anchors have to be the OUTER wrapper --
  // applied before find, never after it -- or closing find would silently strip
  // every anchor it happened to overlap.
  function refreshHighlights() {
    clearFindHighlights();
    applyCommentAnchors();
    refreshFind();
    layoutCommentRail();
    // Last, and only here: the wrapping above has finished splitting text
    // nodes, so this is the first moment an offset can be turned back into a
    // rectangle. restoreCursor puts it where the document says it belongs --
    // the text may have been edited under us since the last motion.
    restoreCursor(cachedIndex());
    placeCaret();
    // The selection's nodes went out with the marks; the offsets that describe
    // it did not.
    if (visual) paintVisual();
    // Last of all: the comment and find marks the map plots exist only now.
    scheduleMinimapPaint();
  }

  function anchorMarks(id) {
    for (var i = 0; i < commentAnchors.length; i++) {
      if (commentAnchors[i].id === id) return commentAnchors[i].marks;
    }
    return null;
  }

  // The comment you are looking at, by the rule z already uses for images.
  // `limit` caps how far off centre it may be: x deletes without undo, so it
  // refuses when there is nothing on screen you could have seen.
  function nearestComment(limit) {
    var best = null;
    var bestDistance = Infinity;
    var centre = window.innerHeight / 2;
    for (var i = 0; i < commentAnchors.length; i++) {
      var marks = commentAnchors[i].marks;
      if (!marks.length) continue;
      var rect = marks[0].getBoundingClientRect();
      var distance = Math.abs((rect.top + rect.bottom) / 2 - centre);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = commentAnchors[i].id;
      }
    }
    if (best === null) return null;
    if (limit != null && bestDistance > limit) return null;
    return commentById(best);
  }

  function commentById(id) {
    for (var i = 0; i < comments.length; i++) {
      if (comments[i].id === id) return comments[i];
    }
    return null;
  }

  // ---- The comment entry bar ------------------------------------------------

  function commentBarEl() {
    return document.getElementById("mdview-comment");
  }

  function commentInputEl() {
    return document.getElementById("mdview-comment-input");
  }

  function commentBarIsOpen() {
    var bar = commentBarEl();
    return !!bar && !bar.hidden;
  }

  function openCommentBar(quote, note, viewportTop) {
    var bar = commentBarEl();
    var input = commentInputEl();
    if (!bar || !input) return;
    // Reached only once captureSelection has already read the selection --
    // commentKey calls it first, and says so. So `c` in visual mode needs no
    // special case at all: the words are captured, then the mode is left.
    exitVisual();
    var label = document.getElementById("mdview-comment-quote");
    // The stylesheet clamps this to one ellipsised line, but the node would
    // still HOLD the whole passage -- which is what the accessibility tree
    // reads out and what a copy taken out of the bar would carry.
    if (label) label.textContent = excerpt(quote, 200);
    input.value = note || "";
    var geometry = viewportTop == null ? null : railGeometry();
    if (geometry) {
      // Level with the passage, in the rail's column: a draft that appears
      // where its card will appear.
      bar.classList.add("is-railed");
      bar.style.left = geometry.left + "px";
      bar.style.width = geometry.width + "px";
      draftTop = viewportTop - geometry.top;
      bar.style.top = draftTop + "px";
    } else {
      draftTop = null;
      bar.classList.remove("is-railed");
      bar.style.left = "";
      bar.style.width = "";
      bar.style.top = "";
    }
    bar.hidden = false;
    layoutCommentRail();
    input.focus();
    input.select();
  }

  function closeCommentBar() {
    var bar = commentBarEl();
    var input = commentInputEl();
    pendingComment = null;
    editingCommentId = null;
    if (!bar || !input) return;
    bar.hidden = true;
    draftTop = null;
    bar.classList.remove("is-railed");
    bar.style.left = "";
    bar.style.width = "";
    bar.style.top = "";
    layoutCommentRail();
    // Blurred on the way out: a hidden field that still holds focus goes on
    // eating j and k. Same hazard the find bar documents.
    input.blur();
  }

  function commitComment() {
    var input = commentInputEl();
    if (!input) return;
    var note = input.value;
    if (editingCommentId !== null) {
      postToHost("editComment:" + editingCommentId + ":" + encodeURIComponent(note));
    } else if (pendingComment) {
      postToHost(
        "addComment:" +
          pendingComment.heading +
          ":" +
          pendingComment.nth +
          ":" +
          encodeURIComponent(pendingComment.quote) +
          ":" +
          encodeURIComponent(note)
      );
    }
    closeCommentBar();
  }

  // ---- Capturing a selection ------------------------------------------------

  function insideUnstableSubtree(node) {
    var el = node && node.nodeType === 1 ? node : node && node.parentNode;
    while (el && el !== document.body) {
      var tag = el.tagName ? el.tagName.toLowerCase() : "";
      // Mermaid re-runs and KaTeX rewrites their own subtrees, so text in
      // there is not the same text after the next render.
      if (tag === "svg") return true;
      if (el.classList && (el.classList.contains("katex") || el.classList.contains("katex-mathml"))) {
        return true;
      }
      el = el.parentNode;
    }
    return false;
  }

  // Where a range endpoint falls in the concatenated text, or -1 when the
  // endpoint is not in a text node this indexes -- a word selection can start
  // at the end of the node before the one you clicked in, and a triple-click
  // anchors on the element. The caller recovers by searching for the words.
  function offsetOfPoint(index, node, offset) {
    for (var i = 0; i < index.spans.length; i++) {
      if (index.spans[i].node === node) return index.spans[i].start + offset;
    }
    return -1;
  }

  // What the selection is; null when there is nothing selected, and false when
  // there is a selection this cannot anchor -- in which case the reason has
  // already been shown. The caller has to tell those apart: with no selection
  // c means "show me the comments", and turning a refusal into that as well
  // would answer a complaint by opening a panel nobody asked for.
  //
  // Called BEFORE the input is focused, because focusing collapses the
  // selection in WebKit.
  function captureSelection() {
    var content = contentEl();
    if (!content) return null;
    if (document.documentElement.getAttribute("data-view") === "diff") {
      showNote("Comments belong on the document, not the diff.");
      return false;
    }
    var selection = null;
    try {
      selection = window.getSelection();
    } catch (err) {
      return null;
    }
    if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
    var range = selection.getRangeAt(0);
    if (!content.contains(range.commonAncestorContainer)) return null;
    if (insideUnstableSubtree(range.startContainer)) {
      showNote("Maths and diagrams cannot hold a comment.");
      return false;
    }
    var index = textIndex(content);
    var starts = sectionStarts(index);

    // The words are taken from the INDEX, never from Selection.toString().
    //
    // The two disagree wherever the markdown source was hard wrapped. A
    // paragraph written across two source lines keeps that newline inside its
    // text node, so the index holds "hard wrapped\nacross two lines" while the
    // selection reports the RENDERED text, "hard wrapped across two lines". A
    // quote taken from the selection is then a string the document does not
    // contain, and every search for it fails -- which used to be reported as a
    // paragraph break, from a selection sitting well inside one paragraph.
    //
    // Read out of the index, a quote is findable by construction, which is the
    // only thing applyCommentAnchors will ever ask of it.
    //
    // Trimmed, because a double-click takes the word and whatever whitespace
    // follows it, and neither end of that is part of what you are commenting
    // on. Anchoring on the trimmed words is also what makes the comment
    // survive a reflow that moves the line break.
    var quote = null;
    var at = -1;
    var from = offsetOfPoint(index, range.startContainer, range.startOffset);
    var to = offsetOfPoint(index, range.endContainer, range.endOffset);
    if (from >= 0 && to > from) {
      var slice = index.text.slice(from, to);
      var lead = slice.replace(/^\s+/, "");
      quote = lead.replace(/\s+$/, "");
      at = from + (slice.length - lead.length);
    }
    // An endpoint that is not a text node has no offset in the index -- a
    // triple click hands back the element itself. Those go through the
    // rendered text, matching whitespace to whitespace so a hard wrap cannot
    // fail the search, and still take the index's own words as the quote.
    if (!quote) {
      var rendered = String(selection).replace(/^\s+|\s+$/g, "");
      if (rendered) {
        var pattern = rendered.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/\s+/g, "\\s+");
        var hit = null;
        try {
          hit = new RegExp(pattern).exec(index.text);
        } catch (err) {
          hit = null;
        }
        if (hit) {
          quote = hit[0];
          at = hit.index;
        } else {
          quote = rendered;
          at = index.text.indexOf(rendered);
        }
      }
    }
    if (!quote) return null;
    if (quote.length > COMMENT_QUOTE_MAX) {
      showNote("That selection is too long to comment on.");
      return false;
    }
    if (at < 0) {
      showNote("Those words could not be found to anchor to.");
      return false;
    }
    var heading = 0;
    for (var i = 0; i < starts.length; i++) {
      if (at >= starts[i]) heading = i + 1;
    }
    var section = sectionRange(index, starts, heading);
    var body = index.text.slice(section.from, section.to);
    // Which occurrence of these words this is, counted within the section, so
    // two identical quotes under one heading stay told apart.
    var nth = 0;
    var cursor = 0;
    while (true) {
      var hit = body.indexOf(quote, cursor);
      if (hit < 0 || section.from + hit >= at) break;
      nth++;
      cursor = hit + 1;
    }
    return { heading: heading, nth: nth, quote: quote, top: range.getBoundingClientRect().top };
  }

  // ---- The keys -------------------------------------------------------------

  function commentKey() {
    var capture = captureSelection();
    // Refused, and it has already said why. Opening the panel on top of that
    // would be answering a complaint with something nobody asked for.
    if (capture === false) return;
    if (!capture) {
      // No selection: c is then the third panel, the way o and b are.
      showSidebarTab("comments");
      return;
    }
    if (!hasHost()) {
      showNote("Comments need the app.");
      return;
    }
    pendingComment = capture;
    editingCommentId = null;
    // The sidebar panel only when the rail has nowhere to go. Opening both
    // would be the same list twice, and the sidebar takes its width out of
    // main -- which is the very margin the rail needs, so it would shut the
    // rail as it opened.
    if (!railGeometry()) setSidebar(true, "comments");
    openCommentBar(capture.quote, "", capture.top);
  }

  // e and the card's own button reach the same two functions, so a comment
  // cannot be editable one way and not the other.
  function beginEditComment(comment) {
    pendingComment = null;
    editingCommentId = comment.id;
    focusComment(comment.id, true);
    var marks = anchorMarks(comment.id);
    var top = marks && marks.length ? marks[0].getBoundingClientRect().top : null;
    openCommentBar(comment.quote, comment.note, top);
  }

  function removeComment(comment) {
    // `shown`, not `excerpt`: that name is the funnel above now.
    var shown = excerpt(comment.quote, 40);
    postToHost("deleteComment:" + comment.id);
    showNote("Deleted the comment on “" + shown + "”");
  }

  // Anchored comments in the order they appear on the page. commentAnchors is
  // in wrapping order, which runs backwards, so this cannot lean on it; two
  // comments on one line are separated by their horizontal position, as the
  // rail does.
  function commentsInDocumentOrder() {
    var list = [];
    for (var i = 0; i < commentAnchors.length; i++) {
      var comment = commentById(commentAnchors[i].id);
      var marks = commentAnchors[i].marks;
      if (!comment || !marks.length) continue;
      var rect = marks[0].getBoundingClientRect();
      list.push({ comment: comment, top: rect.top, left: rect.left });
    }
    list.sort(function (a, b) {
      if (a.top !== b.top) return a.top - b.top;
      return a.left - b.left;
    });
    return list;
  }

  // ) and ( step between comments, wrapping at both ends the way n and N do.
  function stepComment(direction) {
    var list = commentsInDocumentOrder();
    if (!list.length) {
      showNote("No comments to step through.");
      return;
    }
    var at = -1;
    for (var i = 0; i < list.length; i++) {
      if (list[i].comment.id === currentCommentId) {
        at = i;
        break;
      }
    }
    var next;
    if (at >= 0) {
      next = (((at + direction) % list.length) + list.length) % list.length;
    } else if (direction > 0) {
      // Nothing focused yet, so enter the list from where you are reading
      // rather than from the top of the document.
      next = 0;
      for (var f = 0; f < list.length; f++) {
        if (list[f].top > 0) {
          next = f;
          break;
        }
      }
    } else {
      next = list.length - 1;
      for (var b = list.length - 1; b >= 0; b--) {
        if (list[b].top < 0) {
          next = b;
          break;
        }
      }
    }
    var target = list[next].comment;
    // The card is level with the passage, so centring the passage brings the
    // card with it; scrolling both would have them fight.
    focusComment(target.id, false);
    var marks = anchorMarks(target.id);
    if (marks && marks.length) marks[0].scrollIntoView({ block: "center" });
  }

  function editCommentKey() {
    var comment = nearestComment(null);
    if (!comment) {
      showNote("No comment to edit.");
      return;
    }
    beginEditComment(comment);
  }

  function deleteCommentKey() {
    // A viewport height off centre: far enough to cover a comment scrolled to
    // either edge, close enough that you cannot delete one you never saw. The
    // button needs no such guard -- you cannot click what you cannot see.
    var comment = nearestComment(window.innerHeight);
    if (!comment) {
      showNote("No comment on screen to delete.");
      return;
    }
    removeComment(comment);
  }

  function copyReviewKey() {
    if (!postToHost("copyReview")) showNote("Comments need the app.");
  }

  // Whether the app is behind this page at all, asked without sending
  // anything: c has to refuse before opening an input that could never save.
  function hasHost() {
    try {
      return !!window.webkit.messageHandlers.mdview;
    } catch (err) {
      return false;
    }
  }


  // ---- The comment rail -----------------------------------------------------
  //
  // Cards in the document's right margin, each level with the passage it is
  // about. The rail lives inside #mdview-main, which is position:relative and
  // as tall as the document, so page-coordinate offsets scroll with the text on
  // their own. The sidebar could not host this: it is position:sticky, so its
  // contents would need the scroll position fed to them by hand.
  //
  // It appears only when the margin is genuinely wide enough. Reserving space
  // for it instead would re-wrap the text column, which moves your place in the
  // document every time a comment is added -- and every comment is still in the
  // sidebar panel, which is where orphans live in any case.

  var RAIL_MIN = 180;
  var RAIL_MAX = 280;
  var RAIL_GAP = 16;
  var RAIL_CARD_GAP = 8;
  var currentCommentId = null;
  var railResizeTimer = null;
  var draftTop = null;

  function mainEl() {
    return document.getElementById("mdview-main");
  }

  function railEl() {
    var rail = document.getElementById("mdview-comment-rail");
    if (rail) return rail;
    var main = mainEl();
    if (!main) return null;
    rail = document.createElement("div");
    rail.id = "mdview-comment-rail";
    rail.hidden = true;
    // Appended to main rather than to #mdview-content: the body swap on every
    // live reload would take it, and the card you were reading, with it.
    main.appendChild(rail);
    return rail;
  }

  // How much room the margin has, and how wide the rail may be in it.
  function railGeometry() {
    var main = mainEl();
    var content = contentEl();
    if (!main || !content) return null;
    var mainRect = main.getBoundingClientRect();
    var contentRect = content.getBoundingClientRect();
    // Minus the strip: it is fixed to the window rather than in the layout,
    // so main's own right edge is not where the free margin ends.
    var margin = mainRect.right - minimapReserve() - contentRect.right - RAIL_GAP;
    if (margin < RAIL_MIN) return null;
    return {
      top: mainRect.top,
      left: contentRect.right - mainRect.left + RAIL_GAP,
      width: Math.min(RAIL_MAX, margin),
    };
  }

  // Where a comment's highlight sits, in the rail's own coordinates. `left` is
  // carried for the tie-break alone: two comments on the same line share a top,
  // and without it they would keep the order they were made in.
  function anchorPoint(id, geometry) {
    var marks = anchorMarks(id);
    if (!marks || !marks.length) return null;
    var rect = marks[0].getBoundingClientRect();
    return { top: rect.top - geometry.top, left: rect.left };
  }

  function commentCardButton(label, hint, glyph, run) {
    var button = document.createElement("button");
    button.className = "mdview-comment-card-btn";
    button.setAttribute("aria-label", label);
    button.setAttribute("title", label + " (" + hint + ")");
    button.textContent = glyph;
    button.addEventListener("click", function (event) {
      // The card's own click scrolls to the passage; a button press is not
      // that, and the two firing together would fight over the scroll.
      event.stopPropagation();
      run();
    });
    return button;
  }

  function buildCommentCard(comment) {
    var card = document.createElement("div");
    card.className = "mdview-comment-card";
    card.setAttribute("data-comment-id", comment.id);
    // No role="button": the card holds buttons of its own, and a control
    // inside a control is not something a screen reader can announce.
    card.setAttribute("tabindex", "0");
    var note = document.createElement("p");
    note.className = "mdview-comment-card-note";
    // textContent, never innerHTML: the note is document text.
    note.textContent = comment.note || "No note";
    if (!comment.note) note.classList.add("is-empty");
    var actions = document.createElement("div");
    actions.className = "mdview-comment-card-actions";
    actions.appendChild(
      commentCardButton("Edit comment", "e", "\u270E", function () {
        beginEditComment(comment);
      })
    );
    actions.appendChild(
      commentCardButton("Delete comment", "x", "\u2715", function () {
        removeComment(comment);
      })
    );
    card.appendChild(actions);
    card.appendChild(note);
    card.addEventListener("click", function () {
      focusComment(comment.id, false);
      var marks = anchorMarks(comment.id);
      if (marks && marks.length) marks[0].scrollIntoView({ block: "center" });
    });
    // Without the quote on the card, hovering it is what says which passage it
    // belongs to.
    card.addEventListener("mouseenter", function () {
      peekComment(comment.id);
    });
    card.addEventListener("mouseleave", function () {
      peekComment(null);
    });
    return card;
  }

  // Rebuilt wholesale on every render rather than patched, like the outline:
  // incremental updates over already-processed nodes are the defect class that
  // has bitten this project twice.
  function layoutCommentRail() {
    var rail = railEl();
    if (!rail) return;
    var geometry = railGeometry();
    if (!geometry || !commentAnchors.length) {
      rail.hidden = true;
      rail.textContent = "";
      return;
    }
    rail.textContent = "";
    rail.hidden = false;
    rail.style.left = geometry.left + "px";
    rail.style.width = geometry.width + "px";

    var placed = [];
    for (var i = 0; i < commentAnchors.length; i++) {
      var id = commentAnchors[i].id;
      var comment = commentById(id);
      if (!comment) continue;
      // The one being edited is represented by the draft, not by a card.
      if (id === editingCommentId) continue;
      var point = anchorPoint(id, geometry);
      if (!point) continue;
      placed.push({ comment: comment, desired: point.top, left: point.left });
    }
    // The open draft takes a slot of its own so the cards make room for it.
    // It is never moved: it is pinned to the passage you selected, and having
    // it slide out from under the cursor while you type would be worse than a
    // card overlapping it.
    var bar = commentBarEl();
    if (draftTop !== null && bar && !bar.hidden) {
      placed.push({ draft: bar, desired: draftTop, left: -1 });
    }
    // Sorted before anything is appended, so the DOM order is the reading
    // order: commentAnchors runs backwards through the document (wrapping has
    // to, or splitting a text node would invalidate the offsets still to be
    // used), and tab order would follow it.
    placed.sort(function (a, b) {
      if (a.desired !== b.desired) return a.desired - b.desired;
      return a.left - b.left;
    });
    for (var c = 0; c < placed.length; c++) {
      if (placed[c].draft) {
        placed[c].card = placed[c].draft;
        continue;
      }
      var card = buildCommentCard(placed[c].comment);
      if (placed[c].comment.id === currentCommentId) card.classList.add("is-current");
      rail.appendChild(card);
      placed[c].card = card;
    }
    // Level with its passage where it can be, pushed down where two would
    // overlap. Heights are only measurable once the cards are in the document,
    // hence the second pass.
    var floor = 0;
    for (var p = 0; p < placed.length; p++) {
      var top = placed[p].draft ? placed[p].desired : Math.max(placed[p].desired, floor);
      if (!placed[p].draft) placed[p].card.style.top = top + "px";
      floor = top + placed[p].card.offsetHeight + RAIL_CARD_GAP;
    }
  }

  // Hovering a card lights up the passage it is about, without disturbing
  // which comment is actually focused.
  function peekComment(id) {
    for (var a = 0; a < commentAnchors.length; a++) {
      var marks = commentAnchors[a].marks;
      for (var m = 0; m < marks.length; m++) {
        marks[m].classList.toggle("is-peek", id !== null && commentAnchors[a].id === id);
      }
    }
  }

  // Clicking a highlight brings its card forward, and the other way round.
  function focusComment(id, scrollCard) {
    currentCommentId = id;
    var rail = document.getElementById("mdview-comment-rail");
    if (rail) {
      var cards = rail.querySelectorAll(".mdview-comment-card");
      for (var i = 0; i < cards.length; i++) {
        var match = cards[i].getAttribute("data-comment-id") === id;
        cards[i].classList.toggle("is-current", match);
        if (match && scrollCard) cards[i].scrollIntoView({ block: "nearest" });
      }
    }
    for (var a = 0; a < commentAnchors.length; a++) {
      var marks = commentAnchors[a].marks;
      for (var m = 0; m < marks.length; m++) {
        marks[m].classList.toggle("is-current", commentAnchors[a].id === id);
      }
    }
  }

  // ---- Host hooks -----------------------------------------------------------

  window.mdviewSetComments = function (items) {
    comments = Array.isArray(items) ? items : [];
    refreshHighlights();
    if (sidebarTab === "comments") renderSidebarBody();
  };

  function attachCommentListeners() {
    var content = contentEl();
    if (content) {
      content.addEventListener("click", function (event) {
        var el = event.target;
        while (el && el !== content) {
          if (el.classList && el.classList.contains("mdview-comment-anchor")) {
            focusComment(el.getAttribute("data-comment-id"), true);
            return;
          }
          el = el.parentNode;
        }
      });
    }
    // The margin the rail needs comes and goes with the window and with the
    // sidebar, so the cards are re-placed rather than left hanging.
    window.addEventListener("resize", function () {
      clearTimeout(railResizeTimer);
      railResizeTimer = setTimeout(function () {
        layoutCommentRail();
        // The text re-wrapped, so a different heading may be the one above the
        // line even though nobody scrolled -- and every block the minimap drew
        // is somewhere else.
        schedulePositionSync();
        scheduleMinimapPaint();
        // The text re-wrapped, so the offset the caret sits on is somewhere
        // else on screen even though it is the same offset.
        placeCaret();
        // The labels were placed against rects measured before the reflow and
        // now point at the wrong words. Cancelling is honest; moving them
        // under the reader's fingers mid-jump is not.
        if (jumpIsActive()) endJump();
      }, 120);
    });
    var input = commentInputEl();
    if (!input) return;
    input.addEventListener("keydown", function (event) {
      if (event.key === "Enter") {
        event.preventDefault();
        commitComment();
      } else if (event.key === "Escape") {
        event.preventDefault();
        closeCommentBar();
      }
    });
    // The safety net find has for the same reason: the bar can lose focus to a
    // click and Escape should still shut it.
    document.addEventListener("keydown", function (event) {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === "Escape" && commentBarIsOpen() && !zoomState) {
        event.preventDefault();
        closeCommentBar();
      }
    });
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

  // ---- The cursor ----------------------------------------------------------
  //
  // A position in the DOCUMENT, not in the DOM: an integer offset into the
  // concatenated text `textIndex` builds. That is the whole trick. Both
  // highlight layers split text nodes to wrap a match and call normalize() to
  // merge them back, so every (node, offset) pair in the page is invalidated on
  // any render -- while the string those offsets index into is untouched by all
  // of it. Only the node map goes stale, never the position.
  //
  // Across a live reload the string itself changes, so the cursor is also
  // remembered the way a comment anchor is (section ordinal + offset within the
  // section) and re-derived afterwards: an edit somewhere else in the document
  // then does not drag it.

  // How many line-heights j/k will probe before giving up and falling back to
  // the next block. A tall image or a display formula between two lines of text
  // is the case this exists for.
  var CURSOR_PROBES = 8;
  // Keeps the caret this far inside the viewport, and keeps a j/k probe off the
  // very edge where caretRangeFromPoint has nothing to hit.
  var CURSOR_MARGIN = 28;

  var cursorAt = null;      // offset into cachedIndex().text; null before first use
  var cursorAnchor = null;  // { heading, offset } -- survives a reload
  var cursorGoalX = null;   // desired x for j/k; vim's curswant
  // Until when a scroll the cursor itself asked for is still animating. Only
  // g g and G need it: every other cursor scroll is instant, and lands with
  // the cursor already inside the margins, where the follow is a no-op.
  var cursorLedScrollUntil = 0;
  var indexCache = null;
  var blockCache = null;
  var sectionCache = null;

  function cachedIndex() {
    var content = contentEl();
    if (!content) return null;
    if (!indexCache) indexCache = textIndex(content);
    return indexCache;
  }

  function cachedSections(index) {
    if (!sectionCache) sectionCache = sectionStarts(index);
    return sectionCache;
  }

  // Called by everything that splits or merges a text node. Rebuilding is
  // cheaper than reasoning about which spans moved, and the whole point of the
  // offset model is that nothing above this line has to care.
  function invalidateTextIndex() {
    indexCache = null;
    blockCache = null;
    sectionCache = null;
  }

  var BLOCK_TAGS = /^(P|H[1-6]|LI|BLOCKQUOTE|PRE|TD|TH|DT|DD|FIGCAPTION|DIV|SECTION|ARTICLE)$/;

  function nearestBlock(node) {
    var el = node.parentNode;
    while (el && el !== document.body) {
      if (el.tagName && BLOCK_TAGS.test(el.tagName)) return el;
      el = el.parentNode;
    }
    return null;
  }

  // Where each block begins in the concatenated text.
  //
  // This is not a nicety. `index.text` is a raw nodeValue concatenation, and
  // while the renderer does put a newline between one top-level block and the
  // next, it puts nothing at all between the cells of a table row:
  // "<td>one</td><td>two</td>" reads as "onetwo". Without these boundaries a
  // word motion steps over that join as though it were one word.
  //
  // Kept beside the index rather than inside it: applyCommentAnchors counts
  // occurrences, and every comment already stored on disk was anchored against
  // the string exactly as textIndex builds it today.
  function blockBoundaries(index) {
    if (blockCache) return blockCache;
    var bounds = [];
    var previous = null;
    for (var i = 0; i < index.spans.length; i++) {
      var block = nearestBlock(index.spans[i].node);
      if (block !== previous) {
        bounds.push(index.spans[i].start);
        previous = block;
      }
    }
    blockCache = bounds;
    return bounds;
  }

  function blockStartAt(index, at) {
    var bounds = blockBoundaries(index);
    var from = 0;
    for (var i = 0; i < bounds.length; i++) {
      if (bounds[i] <= at) from = bounds[i];
      else break;
    }
    return from;
  }

  function blockEndAfter(index, at) {
    var bounds = blockBoundaries(index);
    for (var i = 0; i < bounds.length; i++) {
      if (bounds[i] > at) return bounds[i];
    }
    return index.text.length;
  }

  // ---- Geometry ------------------------------------------------------------

  // The caret rectangle for an offset. getClientRects()[0], NOT
  // getBoundingClientRect(): a range that straddles a line wrap has a union box
  // two lines tall, and the caret would be drawn down both of them.
  function rectFor(index, at) {
    if (!index || !index.text.length) return null;
    var tail = at >= index.text.length;
    var from = tail ? index.text.length - 1 : at;
    var runs = runsFor(index, from, from + 1);
    if (!runs.length) return null;
    var run = runs[0];
    var range = document.createRange();
    try {
      range.setStart(run.node, run.start);
      range.setEnd(run.node, run.end);
    } catch (err) {
      return null;
    }
    var rects = range.getClientRects();
    if (!rects.length) return null;
    var rect = rects[0];
    // The width is the character's own advance, because the cursor is a block
    // sitting ON the character rather than a bar between two of them. Past the
    // last character there is nothing to measure, so it borrows that one's
    // width; a zero-width glyph gets a floor so the block never disappears.
    var width = Math.max(rect.width, 3);
    return {
      left: tail ? rect.right : rect.left,
      top: rect.top,
      height: rect.height,
      width: width,
    };
  }

  // WebKit's own API, and the reason j/k can be O(1) rather than a search over
  // offsets: it answers "what is at this point" for wrapped lines, table cells
  // and code blocks alike. Feature-checked, so --print-html output opened in a
  // browser without it still gets the word motions, which need no geometry.
  function offsetFromPoint(index, x, y) {
    if (!document.caretRangeFromPoint) return -1;
    var range = null;
    try {
      range = document.caretRangeFromPoint(x, y);
    } catch (err) {
      return -1;
    }
    if (!range) return -1;
    var content = contentEl();
    if (!content || !content.contains(range.startContainer)) return -1;
    if (insideUnstableSubtree(range.startContainer)) return -1;
    return offsetOfPoint(index, range.startContainer, range.startOffset);
  }

  // How far to look for an offset that actually paints. The gaps are one or
  // two characters in practice; this is a bound, not a budget.
  var RENDERABLE_SCAN = 64;

  // Not every offset is a place the cursor can be.
  //
  // The document's markup puts a newline between one block and the next --
  // "</p>\n<h2>" -- and that newline is a real text node with a real offset in
  // the index, but it renders NOTHING: getClientRects() on it comes back empty.
  // A cursor that lands there cannot be drawn, and cannot get off again either,
  // because every motion that needs geometry starts by asking for the rectangle
  // it does not have. That was a cursor that vanished at a heading and stayed
  // vanished.
  //
  // So every offset the cursor takes is snapped to one that is painted,
  // preferring the direction it was already travelling.
  function renderableAt(index, at, dir) {
    var last = Math.max(0, index.text.length - 1);
    var start = Math.max(0, Math.min(last, at));
    var first = dir < 0 ? -1 : 1;
    var order = [first, -first];
    for (var o = 0; o < order.length; o++) {
      var step = order[o];
      var i = start;
      for (var n = 0; n <= RENDERABLE_SCAN && i >= 0 && i <= last; n++, i += step) {
        if (rectFor(index, i)) return i;
      }
    }
    return -1;
  }

  // The one place cursorAt is assigned. Everything that moves the cursor goes
  // through here so the snap above cannot be forgotten at a new call site.
  function setCursor(index, at, dir) {
    var to = renderableAt(index, Math.max(0, Math.min(index.text.length - 1, at)), dir);
    if (to < 0) return false;
    cursorAt = to;
    return true;
  }

  // ---- Painting ------------------------------------------------------------

  function caretEl() {
    var caret = document.getElementById("mdview-caret");
    if (caret) return caret;
    var main = mainEl();
    if (!main) return null;
    caret = document.createElement("div");
    caret.id = "mdview-caret";
    caret.hidden = true;
    // Appended to main rather than to #mdview-content, which the body swap on
    // every live reload replaces wholesale. Absolute inside a relative main is
    // document coordinates, so it scrolls with the text on its own -- the same
    // reason the comment rail lives here.
    main.appendChild(caret);
    return caret;
  }

  function placeCaret() {
    var caret = caretEl();
    if (!caret) return;
    var main = mainEl();
    var index = cursorAt === null ? null : cachedIndex();
    var rect = index ? rectFor(index, cursorAt) : null;
    if (!rect || !main) {
      caret.hidden = true;
      return;
    }
    var box = main.getBoundingClientRect();
    caret.style.left = rect.left - box.left + "px";
    caret.style.top = rect.top - box.top + "px";
    caret.style.height = rect.height + "px";
    caret.style.width = rect.width + "px";
    caret.hidden = false;
  }

  // ---- Remembering it across a render --------------------------------------

  function rememberCursor(index) {
    if (cursorAt === null) {
      cursorAnchor = null;
      return;
    }
    var starts = cachedSections(index);
    var heading = 0;
    for (var i = 0; i < starts.length; i++) {
      if (cursorAt >= starts[i]) heading = i + 1;
    }
    var section = sectionRange(index, starts, heading);
    cursorAnchor = { heading: heading, offset: cursorAt - section.from };
  }

  // The document may have been edited under us. Put the cursor back in the same
  // place in the same section, and clamp rather than drop it: a cursor that
  // vanished on save would be worse than one that moved a word.
  function restoreCursor(index) {
    if (!cursorAnchor || !index) return;
    var starts = cachedSections(index);
    var section = sectionRange(index, starts, cursorAnchor.heading);
    var at = section.from + cursorAnchor.offset;
    // Snapped as well as clamped: an edit can leave the remembered offset on
    // one of the newlines between two blocks.
    setCursor(index, Math.min(at, section.to), 1);
  }

  // Seeded from what is on screen, so the first motion starts where you are
  // reading rather than at the top of a document you may be halfway down.
  function ensureCursor(index) {
    if (cursorAt !== null) return true;
    if (!index || !index.text.length) return false;
    var content = contentEl();
    var box = content ? content.getBoundingClientRect() : null;
    var at = box ? offsetFromPoint(index, box.left + 4, CURSOR_MARGIN) : -1;
    if (!setCursor(index, at >= 0 ? at : 0, 1)) return false;
    cursorGoalX = null;
    return true;
  }

  function scrollCursorIntoView() {
    var index = cachedIndex();
    var rect = index === null ? null : rectFor(index, cursorAt);
    if (!rect) return;
    var top = CURSOR_MARGIN;
    var bottom = window.innerHeight - CURSOR_MARGIN - rect.height;
    // Instant, for the reason scrollLines is: a held j queues one smooth
    // animation per repeat and they fight each other.
    if (rect.top < top) window.scrollBy(0, rect.top - top);
    else if (rect.top > bottom) window.scrollBy(0, rect.top - bottom);
  }

  // The other half of scrollCursorIntoView: there, the cursor moved and the
  // view followed; here the view moved and the cursor follows it. Vim's ⌃d and
  // ⌃e carry the cursor along rather than leaving it behind, and a wheel, a
  // half page or a drag on the minimap is the same movement by another means --
  // after any of them, j should carry on from what you are looking at rather
  // than from a paragraph you scrolled past a page ago.
  //
  // Only from the edges: the cursor is dragged to the margin it crossed, never
  // recentred, so a scroll of two lines does not move it at all.
  function followScrollWithCursor() {
    // Not for a reader who has not used the cursor. The caret is opt-in, and a
    // scroll is not the moment to hand somebody one.
    if (cursorAt === null) return;
    // Not in visual mode: scrolling to see where a selection reaches must not
    // extend it. That selection is what c and y are about to act on.
    if (visual) return;
    // Not behind the lightbox, which owns the screen and the scroll both.
    var overlay = document.getElementById("mdview-lightbox");
    if (overlay && !overlay.hidden) return;
    // Not while g g or G is in flight: those set the offset FIRST and animate
    // the view to it, so a follow mid-animation would drop the cursor wherever
    // the scroll had got to.
    if (Date.now() < cursorLedScrollUntil) return;
    var index = cachedIndex();
    var rect = index === null ? null : rectFor(index, cursorAt);
    if (!rect) return;
    var top = CURSOR_MARGIN;
    var bottom = window.innerHeight - CURSOR_MARGIN - rect.height;
    var y;
    if (rect.top < top) y = top;
    else if (rect.top > bottom) y = bottom;
    else return;
    // Probed at the column it already had, so a cursor dragged down a page
    // comes back on the same side of the text it left on -- and at the middle
    // of the line, which is where caretRangeFromPoint has something to hit.
    var at = offsetFromPoint(index, rect.left, y + rect.height / 2);
    if (at < 0) return;
    if (!setCursor(index, at, at < cursorAt ? -1 : 1)) return;
    rememberCursor(index);
    placeCaret();
  }

  // ---- Motions -------------------------------------------------------------
  //
  // Every motion is (index, at) -> new offset, or -1 for "nothing to do". The
  // wrapper below is what actually moves the cursor, so scrolling, repainting
  // and re-anchoring are written once.

  // vim's three character classes. A WORD (W/E/B) is any run of non-whitespace;
  // a word also breaks between letters and punctuation. Non-ASCII counts as a
  // letter, so a motion does not stop inside an accented or CJK word.
  function charClass(ch, big) {
    if (!ch || /\s/.test(ch)) return 0;
    if (big) return 1;
    return /[A-Za-z0-9_]/.test(ch) || ch.charCodeAt(0) > 127 ? 1 : 2;
  }

  // A block boundary reads as the start of the next word, so `w` steps out of a
  // paragraph onto the first word of the next one exactly as it steps between
  // words -- without the two ever being read as joined.
  function wordForward(index, at, big) {
    var text = index.text;
    var limit = blockEndAfter(index, at);
    var cls = charClass(text.charAt(at), big);
    var i = at;
    while (i < limit && cls !== 0 && charClass(text.charAt(i), big) === cls) i++;
    while (i < limit && charClass(text.charAt(i), big) === 0) i++;
    return Math.min(i, text.length);
  }

  function wordEnd(index, at, big) {
    var text = index.text;
    var i = at + 1;
    // The whitespace skip is NOT bounded by the block: `e` on the last word of
    // a paragraph goes to the end of the first word of the next one, the way it
    // crosses a line in vim. Bounding it here is what made `e` stick.
    while (i < text.length && charClass(text.charAt(i), big) === 0) i++;
    if (i >= text.length) return Math.max(at, text.length - 1);
    // The run itself is bounded, by the block the landing character is in.
    var cls = charClass(text.charAt(i), big);
    var stop = blockEndAfter(index, i);
    while (i + 1 < stop && charClass(text.charAt(i + 1), big) === cls) i++;
    return i;
  }

  function wordBack(index, at, big) {
    var text = index.text;
    var floor = blockStartAt(index, at);
    var i = at;
    // Already at the top of the block: drop into the one before it.
    if (i <= floor) {
      if (floor <= 0) return 0;
      i = floor;
      floor = blockStartAt(index, floor - 1);
    }
    i--;
    while (i > floor && charClass(text.charAt(i), big) === 0) i--;
    var cls = charClass(text.charAt(i), big);
    while (i > floor && charClass(text.charAt(i - 1), big) === cls) i--;
    return Math.max(floor, i);
  }

  // Clamped to the block rather than to the visual line. A rendered paragraph
  // re-wraps with the window, so "the end of the line" is not a place in the
  // document; the end of the paragraph is.
  function charStep(index, at, delta) {
    var from = blockStartAt(index, at);
    var to = Math.max(from, blockEndAfter(index, at) - 1);
    return Math.max(from, Math.min(to, at + delta));
  }

  // Down and up a VISUAL line -- vim's gj/gk, which is the only reading that
  // makes sense in reflowed prose. Probing rather than one shot because the gap
  // between two blocks, a tall image or a display formula all land nowhere.
  function lineStep(index, at, dir) {
    var rect = rectFor(index, at);
    if (!rect) return -1;
    if (cursorGoalX === null) cursorGoalX = rect.left;
    var step = Math.max(6, Math.round(rect.height));
    var y = rect.top + rect.height / 2;
    for (var i = 0; i < CURSOR_PROBES; i++) {
      y += dir * step;
      // caretRangeFromPoint works in VIEWPORT coordinates, so a target below
      // the fold cannot be hit at all until it is scrolled into view.
      var over = y < CURSOR_MARGIN ? y - CURSOR_MARGIN : 0;
      if (y > window.innerHeight - CURSOR_MARGIN) over = y - (window.innerHeight - CURSOR_MARGIN);
      if (over !== 0) {
        window.scrollBy(0, over);
        y -= over;
      }
      var hit = offsetFromPoint(index, cursorGoalX, y);
      if (hit >= 0 && hit !== at) return hit;
    }
    // Nothing hittable that way: step to the next block, so j never dead-ends
    // against a diagram.
    return dir > 0 ? blockEndAfter(index, at) : Math.max(0, blockStartAt(index, at) - 1);
  }

  // Ends of the visual line, answered by geometry for the same reason j/k are:
  // scanning character by character would cost a layout per character.
  function lineEdge(index, at, dir) {
    var rect = rectFor(index, at);
    var content = contentEl();
    if (!rect || !content) return -1;
    var box = content.getBoundingClientRect();
    var y = rect.top + rect.height / 2;
    var x = dir < 0 ? box.left + 1 : box.right - 1;
    var hit = offsetFromPoint(index, x, y);
    return hit >= 0 ? hit : -1;
  }

  // The one place the cursor actually moves.
  function cursorKey(motion) {
    return function () {
      var index = cachedIndex();
      if (!index || !index.text.length) return;
      if (!ensureCursor(index)) return;
      var at = motion(index, cursorAt);
      if (at < 0) return;
      // Never past the last character, and never on one that paints nothing:
      // the cursor is a block sitting ON a character, which is vim's
      // normal-mode invariant and the only position a block can be drawn.
      if (!setCursor(index, at, at < cursorAt ? -1 : 1)) return;
      rememberCursor(index);
      // Scroll first: the caret is positioned in DOCUMENT coordinates, so one
      // placement after the scroll is both correct and enough.
      scrollCursorIntoView();
      placeCaret();
      // The head moved, so the selection did. Visual mode adds exactly this
      // one line to the motion path -- the motions themselves know nothing
      // about it.
      if (visual) paintVisual();
    };
  }

  // The ends of the document keep their SMOOTH scroll -- a jump this long is
  // the one case where the animation tells you which way you travelled -- but
  // they carry the cursor with them like any other motion.
  function jumpCursorTo(toEnd) {
    var index = cachedIndex();
    if (index && index.text.length) {
      setCursor(index, toEnd ? index.text.length - 1 : 0, toEnd ? -1 : 1);
      cursorGoalX = null;
      rememberCursor(index);
    }
    // Long enough to outlast the smooth scroll, and the same window the
    // heading chain uses for the same reason.
    cursorLedScrollUntil = Date.now() + HEADING_CHAIN_MS;
    scrollToY(toEnd ? maxScrollY() : 0);
    placeCaret();
  }

  // Horizontal motion sets the column j/k will aim for; vertical motion keeps
  // whatever the last horizontal one asked for, which is what stops a j through
  // a short line from dragging the cursor left for good.
  function cursorKeyX(motion) {
    var move = cursorKey(motion);
    return function () {
      cursorGoalX = null;
      move();
    };
  }


  // ---- Visual mode ---------------------------------------------------------
  //
  // The selection is a REAL DOM Selection, never a third <mark> layer. The two
  // wrapping layers this page already has are ordered against each other --
  // comment anchors outside, find hits inside -- because clearFindHighlights
  // unwraps a mark by replacing it with a flat text node, which deletes
  // whatever was nested inside. There is no third slot to be had, and a Range
  // needs none: it costs no DOM at all, and `c` already reads exactly this.
  //
  // Which is the point. captureSelection() does not learn a new trick here; it
  // is handed the selection it has always read, made by the keyboard instead of
  // by the mouse.

  var visual = null; // { anchor: offset, block: bool }

  function visualIsOn() {
    return visual !== null;
  }

  // The head is the cursor, so every motion extends the selection for free.
  // Inclusive of the character under the head -- the cursor is a block sitting
  // on a character, and vim selects the one it is sitting on.
  function visualRange(index) {
    if (!visual) return null;
    var from = Math.min(visual.anchor, cursorAt);
    var to = Math.min(index.text.length, Math.max(visual.anchor, cursorAt) + 1);
    if (visual.block) {
      from = blockStartAt(index, from);
      to = blockEndAfter(index, to - 1);
    }
    return { from: from, to: to };
  }

  function paintVisual() {
    var index = cachedIndex();
    if (!index || !visual) return;
    var span = visualRange(index);
    if (!span || span.to <= span.from) return;
    var runs = runsFor(index, span.from, span.to);
    if (!runs.length) return;
    var selection = null;
    try {
      selection = window.getSelection();
    } catch (err) {
      return;
    }
    if (!selection) return;
    var range = document.createRange();
    var last = runs[runs.length - 1];
    try {
      range.setStart(runs[0].node, runs[0].start);
      range.setEnd(last.node, last.end);
    } catch (err) {
      return;
    }
    // WebKit scrolls to a range it is handed. The motion that got us here has
    // already put the viewport where it belongs, so put it back -- the same
    // save-and-restore the live reload does around its body swap.
    var y = window.scrollY;
    selection.removeAllRanges();
    selection.addRange(range);
    if (window.scrollY !== y) window.scrollTo(0, y);
  }

  function enterVisual(block) {
    var index = cachedIndex();
    if (!index || !index.text.length) return;
    if (!ensureCursor(index)) return;
    visual = { anchor: cursorAt, block: !!block };
    placeCaret();
    paintVisual();
  }

  // Returns whether there was anything to leave, and that matters more than it
  // looks: mdviewOpenFind seeds its query from the document selection, and a
  // selection made with the MOUSE has to survive being asked. Only a selection
  // this mode painted is cleared, so `/` after a double-click still seeds while
  // `/` after `v` opens empty, the way it does in vim.
  function exitVisual() {
    if (!visual) return false;
    visual = null;
    try {
      var selection = window.getSelection();
      if (selection) selection.removeAllRanges();
    } catch (err) {
      /* no selection API: there was nothing painted to clear */
    }
    placeCaret();
    return true;
  }

  function toggleVisual(block) {
    if (visual && !!visual.block === !!block) {
      exitVisual();
      return;
    }
    if (visual) {
      visual.block = !!block;
      paintVisual();
      return;
    }
    enterVisual(block);
  }

  function swapVisualEnds() {
    if (!visual) return;
    var index = cachedIndex();
    if (!index) return;
    var head = cursorAt;
    if (!setCursor(index, visual.anchor, 1)) return;
    visual.anchor = head;
    cursorGoalX = null;
    rememberCursor(index);
    scrollCursorIntoView();
    placeCaret();
    paintVisual();
  }

  // execCommand rather than a host message: the selection is right there, it
  // needs no new wire format for the bridge to learn, and it works in
  // --print-html output opened in a plain browser. A keydown handler is the
  // user gesture it wants.
  function copyVisual() {
    if (!visual) return;
    var copied = false;
    try {
      copied = document.execCommand("copy");
    } catch (err) {
      copied = false;
    }
    exitVisual();
    showNote(copied ? "Copied." : "Nothing copied.");
  }


  // ---- Jump ----------------------------------------------------------------
  //
  // `s`, then type what you are looking at. Every occurrence on screen lights
  // up as you type and the nearest ones take a label; type more to narrow, or
  // type a label to go there. Backspace takes a character back, enter takes the
  // nearest match, esc gives up.
  //
  // The one idea that makes this work without a mode switch: A LABEL IS NEVER A
  // CHARACTER THAT COULD CONTINUE THE SEARCH. The letters immediately following
  // the current matches are struck out of the label alphabet, so a keystroke is
  // never ambiguous -- if it is a label it jumps, and if it is not it narrows.
  // Nothing has to guess which you meant.
  //
  // The labels and the highlights are ordinary spans in their own layer, never
  // <mark>: the two wrapping layers are ordered against each other and there is
  // no third slot, the same reason visual mode paints a Selection.

  // How many matches are drawn at once. A cap on drawing, not on what can be
  // jumped to -- one more character narrows the field.
  var JUMP_MATCH_MAX = 300;
  // Home row first, so the labels that come up most are under the fingers.
  var JUMP_ALPHABET = "asdfghjklqwertyuiopzxcvbnm";

  var jumpOn = false;
  var jumpQuery = "";
  var jumpMatches = []; // [{ at, end, rects, label }]  label is null when unlabelled

  function jumpIsActive() {
    return jumpOn;
  }

  // The slice of the document on screen, bracketed by probing down from the top
  // edge and up from the bottom. Probing rather than one shot because a point
  // in a margin, between two blocks, hits nothing at all.
  function visibleRange(index) {
    var content = contentEl();
    if (!content) return null;
    var box = content.getBoundingClientRect();
    var from = -1;
    var to = -1;
    for (var y = CURSOR_MARGIN; y < window.innerHeight && from < 0; y += 24) {
      from = offsetFromPoint(index, box.left + 4, y);
    }
    for (var z = window.innerHeight - CURSOR_MARGIN; z > 0 && to < 0; z -= 24) {
      to = offsetFromPoint(index, box.right - 4, z);
    }
    if (from < 0) from = 0;
    if (to < 0) to = index.text.length;
    return from <= to ? { from: from, to: to } : { from: to, to: from };
  }

  // Every box a stretch of text occupies -- more than one when it wraps, which
  // is why this is not a single bounding rectangle.
  function rectsFor(index, from, to) {
    var runs = runsFor(index, from, to);
    if (!runs.length) return [];
    var range = document.createRange();
    var last = runs[runs.length - 1];
    try {
      range.setStart(runs[0].node, runs[0].start);
      range.setEnd(last.node, last.end);
    } catch (err) {
      return [];
    }
    var list = range.getClientRects();
    var out = [];
    for (var i = 0; i < list.length; i++) {
      if (list[i].width > 0 && list[i].height > 0) out.push(list[i]);
    }
    return out;
  }

  function collectJumpMatches(index, query) {
    var span = visibleRange(index);
    if (!span) return [];
    var hay = index.text.toLowerCase();
    var needle = query.toLowerCase();
    var raw = [];
    var i = hay.indexOf(needle, span.from);
    while (i >= 0 && i <= span.to && raw.length < JUMP_MATCH_MAX) {
      raw.push(i);
      i = hay.indexOf(needle, i + 1);
    }

    // A match with no boxes is text that paints nothing -- the newline between
    // two blocks is in the index like any other character. Dropping it here is
    // also what keeps a label off a place the cursor could not go.
    var out = [];
    for (var k = 0; k < raw.length; k++) {
      var rects = rectsFor(index, raw[k], raw[k] + needle.length);
      if (!rects.length) continue;
      var head = rects[0];
      if (head.top + head.height < 0 || head.top > window.innerHeight) continue;
      out.push({ at: raw[k], end: raw[k] + needle.length, rects: rects, label: null });
    }

    // Nearest the cursor first: the easiest labels land where you are most
    // likely to be looking, and enter takes the closest match.
    out.sort(function (x, y) {
      return Math.abs(x.at - cursorAt) - Math.abs(y.at - cursorAt);
    });

    // Strike out every letter that could continue the search. This is what
    // lets one keystroke mean either "narrow" or "go" with no ambiguity.
    var continues = {};
    for (var t = 0; t < out.length; t++) {
      var next = index.text.charAt(out[t].end).toLowerCase();
      if (next) continues[next] = true;
    }
    var pool = [];
    for (var a = 0; a < JUMP_ALPHABET.length; a++) {
      var ch = JUMP_ALPHABET.charAt(a);
      if (!Object.prototype.hasOwnProperty.call(continues, ch)) pool.push(ch);
    }
    // A single match gets no label. You are mid-word by then, and a label
    // offered there is a second thing to read and decide about when the thing
    // you were already doing -- typing -- still works. Enter takes it.
    //
    // More matches than labels is fine too: the rest stay lit but unlabelled,
    // and one more character brings them within reach.
    if (out.length > 1) {
      for (var m = 0; m < out.length && m < pool.length; m++) out[m].label = pool[m];
    }
    return out;
  }

  function jumpLayerEl() {
    var layer = document.getElementById("mdview-jump");
    if (layer) return layer;
    var main = mainEl();
    if (!main) return null;
    layer = document.createElement("div");
    layer.id = "mdview-jump";
    // On main, like the caret and the rail: the live reload replaces
    // #mdview-content wholesale and would take the labels with it.
    main.appendChild(layer);
    return layer;
  }

  // Every rect was measured before anything was appended, so the whole set
  // costs one layout rather than one per box.
  function drawJump() {
    var layer = jumpLayerEl();
    var main = mainEl();
    if (!layer || !main) return;
    layer.innerHTML = "";
    if (!jumpMatches.length) return;
    var box = main.getBoundingClientRect();
    var fragment = document.createDocumentFragment();
    for (var i = 0; i < jumpMatches.length; i++) {
      var match = jumpMatches[i];
      for (var r = 0; r < match.rects.length; r++) {
        var rect = match.rects[r];
        var hit = document.createElement("span");
        hit.className = "mdview-jump-hit";
        hit.style.left = rect.left - box.left + "px";
        hit.style.top = rect.top - box.top + "px";
        hit.style.width = rect.width + "px";
        hit.style.height = rect.height + "px";
        fragment.appendChild(hit);
      }
      if (!match.label) continue;
      // After the match, the way flash does it: a label sitting on the words
      // would hide the very text you are reading to aim.
      var tail = match.rects[match.rects.length - 1];
      var label = document.createElement("span");
      label.className = "mdview-jump-label";
      label.textContent = match.label;
      label.style.left = tail.right - box.left + "px";
      label.style.top = tail.top - box.top + "px";
      label.style.height = tail.height + "px";
      fragment.appendChild(label);
    }
    layer.appendChild(fragment);
  }

  function endJump() {
    jumpOn = false;
    jumpQuery = "";
    jumpMatches = [];
    var layer = document.getElementById("mdview-jump");
    if (layer) layer.innerHTML = "";
  }

  function jumpTo(at) {
    var index = cachedIndex();
    endJump();
    if (!index || !index.text.length) return;
    if (!setCursor(index, at, 1)) return;
    cursorGoalX = null;
    rememberCursor(index);
    scrollCursorIntoView();
    placeCaret();
    // In visual mode the jump EXTENDS the selection rather than moving the
    // anchor, because the head is the cursor and nothing else had to change.
    if (visual) paintVisual();
  }

  function setJumpQuery(query) {
    var index = cachedIndex();
    if (!index) {
      endJump();
      return;
    }
    if (!query) {
      jumpQuery = "";
      jumpMatches = [];
      drawJump();
      return;
    }
    var found = collectJumpMatches(index, query);
    if (!found.length) {
      // Refuse the keystroke rather than sitting in a state that matches
      // nothing: what is on screen stays, and the search is still live.
      showNote("Nothing on screen matches " + JSON.stringify(query) + ".");
      return;
    }
    jumpQuery = query;
    jumpMatches = found;
    drawJump();
    // Down to one, and typing is how you got here: say how to take it rather
    // than taking it. Jumping the moment a query happened to be unique used to
    // end the mode mid-word, and the rest of the word you were still typing
    // ran as commands -- which reads exactly like the search resetting itself.
    if (found.length === 1) showNote("One match — enter to go.");
  }

  function beginJump() {
    var index = cachedIndex();
    if (!index || !index.text.length) return;
    if (!ensureCursor(index)) return;
    jumpOn = true;
    jumpQuery = "";
    jumpMatches = [];
    showNote("Jump to…");
  }

  function onJumpKey(event) {
    var key = event.key;
    if (key === "Escape") {
      event.preventDefault();
      endJump();
      return;
    }
    if (key === "Backspace") {
      event.preventDefault();
      if (!jumpQuery) endJump();
      else setJumpQuery(jumpQuery.slice(0, -1));
      return;
    }
    if (key === "Enter") {
      event.preventDefault();
      if (jumpMatches.length) jumpTo(jumpMatches[0].at);
      else endJump();
      return;
    }
    // A modifier or a named key (Shift, ArrowLeft) is not an answer to the
    // question being asked, so it is left alone rather than silently eaten.
    if (key.length !== 1) return;
    event.preventDefault();
    // Labels first, and they can never collide with a character that would
    // narrow the search -- collectJumpMatches struck those out of the pool.
    if (jumpQuery) {
      var lower = key.toLowerCase();
      for (var i = 0; i < jumpMatches.length; i++) {
        if (jumpMatches[i].label === lower) {
          jumpTo(jumpMatches[i].at);
          return;
        }
      }
    }
    setJumpQuery(jumpQuery + key);
  }

  // ---- Keyboard shortcuts -------------------------------------------------
  //
  // Vim-flavoured bindings. A binding is one key ("j"), one control key
  // ("Ctrl+d") or a two-key sequence ("g s"), and all three are written the
  // same way in `keys`. Unmodified letters are safe here because the page has
  // no editing at all and only two text fields (find, comment), so a letter
  // can never be something the user meant to type.
  //
  // ONE table drives the dispatcher, the `?` cheat sheet and the command
  // palette. A binding that exists but is undocumented is therefore not
  // expressible -- which is the whole reason the table exists, since a
  // single-key shortcut leaves no trace in the menu bar to discover it by.
  //
  // `vim: true` marks a row whose key is vim's own -- h, w, /, ⌃f, y. The
  // sheet prints them, because it is the map of the whole keyboard; the
  // palette leaves them out, because a reader who wants "down a line" knows
  // it is j, and twenty-five rows of vim would bury the commands that are
  // MDView's alone.

  var SCROLL_LINE = 60;
  // Two lines of overlap between pages, so nothing is stepped over unread.
  var PAGE_OVERLAP = 2 * SCROLL_LINE;
  // How long a pressed prefix waits for the key that completes it. A prefix
  // SWALLOWS whatever follows, so this window is also the way out of one
  // pressed by accident; esc cancels it outright.
  var CHORD_MS = 700;
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

  var pendingPrefix = null;   // { key, at } -- an unconsumed prefix press
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
    var all = documentHeadings(content);
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
    var known = tab === "bookmarks" || tab === "comments" ? tab : "outline";
    setSidebar(true, known);
  };

  function cycleDiffLayout() {
    var root = document.documentElement;
    // Only meaningful in the diff view: a Markdown render has no unified and
    // split form to choose between.
    if (root.getAttribute("data-view") !== "diff") return;
    // Two axes, four stops: the source or the document, in one column or two.
    var layouts = ["unified", "split", "rendered", "rendered-split"];
    var at = layouts.indexOf(root.getAttribute("data-diff-layout"));
    postToHost("setDiffLayout:" + layouts[(at + 1) % layouts.length]);
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
    // Same condition the View menu item is disabled under: a file with no Git
    // diff to show has nothing to toggle to. Leaving Diff is always allowed,
    // or an unavailable diff would be a one-way door.
    var inDiff = document.documentElement.getAttribute("data-view") === "diff";
    if (!inDiff && !window.mdviewDiffAvailable) {
      // Saying WHY. Refusing in silence made D read as a broken key, which is
      // what it looks like on any file outside a Git repository -- and the
      // menu item being greyed out is no help to someone using the keyboard.
      showNote(diffUnavailableReason || "There is no Git diff for this file.");
      return;
    }
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
        { vim: true, keys: ["h"], hint: "h", label: "Left a character", run: cursorKeyX(function (index, at) { return charStep(index, at, -1); }) },
        { vim: true, keys: ["l"], hint: "l", label: "Right a character", run: cursorKeyX(function (index, at) { return charStep(index, at, 1); }) },
        { vim: true, keys: ["j"], hint: "j", label: "Down a line", run: cursorKey(function (index, at) { return lineStep(index, at, 1); }) },
        { vim: true, keys: ["k"], hint: "k", label: "Up a line", run: cursorKey(function (index, at) { return lineStep(index, at, -1); }) },
        { keys: ["s"], hint: "s", label: "Jump to anything you can see", run: beginJump },
        { vim: true, keys: ["^"], hint: "^", label: "Start of the line", run: cursorKeyX(function (index, at) { return lineEdge(index, at, -1); }) },
        { vim: true, keys: ["$"], hint: "$", label: "End of the line", run: cursorKeyX(function (index, at) { return lineEdge(index, at, 1); }) },
        { vim: true, keys: ["g g"], hint: "g  g", label: "Top of the document", run: function () { jumpCursorTo(false); } },
        { vim: true, keys: ["G"], hint: "G", label: "Bottom of the document", run: function () { jumpCursorTo(true); } },
      ],
    },
    {
      title: "Words",
      items: [
        { vim: true, keys: ["w"], hint: "w", label: "Forward a word", run: cursorKeyX(function (index, at) { return wordForward(index, at, false); }) },
        { vim: true, keys: ["W"], hint: "W", label: "Forward a WORD", run: cursorKeyX(function (index, at) { return wordForward(index, at, true); }) },
        { vim: true, keys: ["e"], hint: "e", label: "To the end of the word", run: cursorKeyX(function (index, at) { return wordEnd(index, at, false); }) },
        { vim: true, keys: ["E"], hint: "E", label: "To the end of the WORD", run: cursorKeyX(function (index, at) { return wordEnd(index, at, true); }) },
        { vim: true, keys: ["b"], hint: "b", label: "Back a word", run: cursorKeyX(function (index, at) { return wordBack(index, at, false); }) },
        { vim: true, keys: ["B"], hint: "B", label: "Back a WORD", run: cursorKeyX(function (index, at) { return wordBack(index, at, true); }) },
      ],
    },
    {
      title: "Selecting",
      items: [
        { vim: true, keys: ["v"], hint: "v", label: "Select from the cursor", run: function () { toggleVisual(false); } },
        { vim: true, keys: ["V"], hint: "V", label: "Select whole blocks", run: function () { toggleVisual(true); } },
        { vim: true, keys: ["o"], hint: "o", label: "Swap which end you are moving", run: swapVisualEnds },
        { vim: true, keys: ["y"], hint: "y", label: "Copy the selection", run: copyVisual },
        { keys: [], hint: "c", label: "Comment on the selection", run: null },
      ],
    },
    {
      title: "Scrolling",
      items: [
        { keys: ["d", "Ctrl+d"], hint: "d", label: "Half a page down", run: function () { scrollLines(halfPage()); } },
        { keys: ["u", "Ctrl+u"], hint: "u", label: "Half a page up", run: function () { scrollLines(-halfPage()); } },
        { vim: true, keys: ["Ctrl+f"], hint: "⌃f", label: "A page down", run: function () { scrollLines(pageStep()); } },
        { vim: true, keys: ["Ctrl+b"], hint: "⌃b", label: "A page up", run: function () { scrollLines(-pageStep()); } },
        { vim: true, keys: ["Ctrl+e"], hint: "⌃e", label: "A line down, leaving the cursor", run: function () { scrollLines(SCROLL_LINE); } },
        { vim: true, keys: ["Ctrl+y"], hint: "⌃y", label: "A line up, leaving the cursor", run: function () { scrollLines(-SCROLL_LINE); } },
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
        { vim: true, keys: ["/"], hint: "/", label: "Find in the document", run: function () { window.mdviewOpenFind(); } },
        { keys: [], hint: "enter", label: "Search, and back to the document", run: null },
        { vim: true, keys: ["n", "Enter"], hint: "n  enter", label: "Next match", run: function () { stepFindKey(1); } },
        { vim: true, keys: ["N"], hint: "N  ⇧enter", label: "Previous match", run: function () { stepFindKey(-1); } },
      ],
    },
    {
      title: "Sidebar",
      items: [
        { keys: ["g s"], hint: "g  s", label: "Toggle the sidebar", run: toggleSidebarKey },
        { keys: ["g o"], hint: "g  o", label: "Outline", run: function () { showSidebarTab("outline"); } },
        { keys: ["g b"], hint: "g  b", label: "Bookmarks", run: function () { showSidebarTab("bookmarks"); } },
        { keys: ["g m"], hint: "g  m", label: "Toggle the minimap", run: toggleMinimapKey },
        { keys: ["m"], hint: "m", label: "Bookmark this document", run: function () { postToHost("toggleBookmark"); } },
        { keys: ["g t"], hint: "g  t", label: "Themes", run: toggleThemePalette },
        { keys: ["g r"], hint: "g  r", label: "Recent files", run: toggleRecentPalette },
      ],
    },
    {
      title: "Comments",
      items: [
        { keys: ["c"], hint: "c", label: "Comment on the selection, or show the comments", run: commentKey },
        { keys: [")"], hint: ")", label: "Next comment", run: function () { stepComment(1); } },
        { keys: ["("], hint: "(", label: "Previous comment", run: function () { stepComment(-1); } },
        { keys: ["g c"], hint: "g  c", label: "Edit the comment you are looking at", run: editCommentKey },
        { keys: ["x"], hint: "x", label: "Delete the comment you are looking at", run: deleteCommentKey },
        { keys: ["C"], hint: "C", label: "Copy the review prompt for Claude", run: copyReviewKey },
      ],
    },
    {
      title: "View",
      items: [
        { keys: ["g d"], hint: "g  d", label: "Diff and back to Markdown", run: toggleDiffKey },
        { keys: ["g l"], hint: "g  l", label: "Diff layout: source or rendered, one column or two", run: cycleDiffLayout },
        { keys: ["z"], hint: "z", label: "Zoom the nearest image", run: zoomNearest },
        { keys: ["g w"], hint: "g  w", label: "Toggle full width", run: toggleFullWidthKey },
        { keys: ["r"], hint: "r", label: "Reload the document", run: reloadKey },
        { keys: ["+", "="], hint: "+", label: "Zoom in", run: function () { postToHost("zoomIn"); } },
        { keys: ["-"], hint: "−", label: "Zoom out", run: function () { postToHost("zoomOut"); } },
        { keys: ["0"], hint: "0", label: "Actual size", run: function () { postToHost("zoomReset"); } },
        { keys: [":"], hint: ":", label: "Run a command by name", run: toggleCommandPalette },
        { keys: ["?"], hint: "?", label: "This list", run: function () { toggleShortcuts(); } },
      ],
    },
    {
      title: "Themes",
      items: [
        { keys: [], hint: "\u2191 \u2193", label: "Move, previewing as you go", run: null },
      ],
    },
    {
      title: "Zoomed image or diagram",
      items: [
        { keys: [], hint: "z", label: "Open it filling the window", run: null },
        { keys: [], hint: "+  −  0", label: "Zoom in, out, reset", run: null },
      ],
    },
  ];

  var keyMap = null;    // "j", "Ctrl+d"  -> item
  var chordMap = null;  // "g s"          -> item
  var prefixSet = null; // "g"            -> true

  // One pass over the table fills all three. A `keys` name containing a space
  // is a two-key sequence, and its first token is a prefix -- so the set of
  // prefixes is DERIVED from the table rather than listed here, and adding
  // "g q" later needs no change to the dispatcher.
  function buildKeyMaps() {
    keyMap = {};
    chordMap = {};
    prefixSet = {};
    for (var g = 0; g < SHORTCUTS.length; g++) {
      var items = SHORTCUTS[g].items;
      for (var i = 0; i < items.length; i++) {
        if (!items[i].run) continue;
        for (var k = 0; k < items[i].keys.length; k++) {
          var name = items[i].keys[k];
          var at = name.indexOf(" ");
          if (at > 0) {
            chordMap[name] = items[i];
            prefixSet[name.slice(0, at)] = true;
          } else {
            keyMap[name] = items[i];
          }
        }
      }
    }
  }

  // hasOwnProperty, not a truth test: a key like "constructor" would
  // otherwise find something on Object.prototype and be "bound".
  function lookup(map, key) {
    return Object.prototype.hasOwnProperty.call(map, key) ? map[key] : null;
  }

  function shortcutFor(key) {
    if (!keyMap) buildKeyMaps();
    return lookup(keyMap, key);
  }

  function chordFor(prefix, key) {
    if (!chordMap) buildKeyMaps();
    return lookup(chordMap, prefix + " " + key);
  }

  function isPrefix(key) {
    if (!prefixSet) buildKeyMaps();
    return lookup(prefixSet, key) === true;
  }

  // Whether a ⌃-combo is one the page has asked for. Everything else modified
  // belongs to the menu bar and to WebKit, and is handed straight back.
  function isCtrlBound(key) {
    if (!keyMap) buildKeyMaps();
    return lookup(keyMap, "Ctrl+" + String(key).toLowerCase()) !== null;
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
    // ⌘ and ⌥ belong to the menu bar (⌘O, ⌘F, ⌘R) and to the browser. ⌃ does
    // not: menu.rs installs no ⌃ equivalent and asserts it, so the scroll keys
    // ⌃d ⌃u ⌃f ⌃b are the page's to claim. Any other ⌃ combo is handed straight
    // back -- WebKit and AppKit have their own uses for it.
    if (event.metaKey || event.altKey) return;
    if (event.ctrlKey && !isCtrlBound(event.key)) return;
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

    // And so are the other two. No two can be up at once: every way of opening
    // any of them goes through this handler, and it returns above.
    if (recentPaletteIsOpen()) {
      onRecentPaletteKey(event);
      return;
    }

    if (commandPaletteIsOpen()) {
      onCommandPaletteKey(event);
      return;
    }

    // So is a jump in progress: every key is either the character being
    // searched for or the label being picked, and nothing else runs until it
    // resolves or is cancelled.
    if (jumpIsActive()) {
      onJumpKey(event);
      return;
    }

    // The find field, the comment input and the palette's search box are the
    // only places in this page where a letter is a letter. This guard has to
    // stay ABOVE the "Ctrl+" naming below: macOS binds ⌃d ⌃u ⌃a ⌃e as emacs
    // editing keys inside a text field, and the page must not steal them.
    if (isTextEntry(event.target) || isTextEntry(document.activeElement)) return;

    if (activatesOnKey(document.activeElement, event.key)) return;

    // Canonical name first, then one lookup. ⇧enter steps the search back and
    // arrives under the unshifted key with shiftKey set, so the shift is read
    // here; ⌃ becomes part of the name the same way.
    var key = event.key;
    if (key === "Enter" && event.shiftKey) key = "N";
    if (event.ctrlKey) key = "Ctrl+" + key.toLowerCase();

    // A live prefix owns the next key whether or not it completes a chord.
    // With eight commands behind g, a mistyped "g" must not fall through and
    // fire an unrelated one -- "g" then "x" used to delete a comment.
    if (pendingPrefix && Date.now() - pendingPrefix.at < CHORD_MS) {
      var chord = chordFor(pendingPrefix.key, key);
      pendingPrefix = null;
      // esc cancels the prefix and nothing else. It is deliberately not
      // swallowed: the find bar and the comment bar have their own esc
      // listeners and still have to see it.
      if (key === "Escape") return;
      event.preventDefault();
      if (chord) chord.run();
      return;
    }
    pendingPrefix = null;

    // Below the modal blocks above, so a sheet or a palette still owns esc
    // first; above nothing else, because find and the comment bar guard their
    // own esc listeners on being open and are no-ops while this is what is up.
    if (key === "Escape" && visualIsOn()) {
      event.preventDefault();
      exitVisual();
      return;
    }

    if (isPrefix(key)) {
      pendingPrefix = { key: key, at: Date.now() };
      event.preventDefault();
      return;
    }

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
    // The map is painted pixels, not styled elements: nothing about a theme
    // change reaches it unless it is repainted.
    scheduleMinimapPaint();
  }

  function attachMinimapListeners() {
    var el = minimapEl();
    if (!el) return;
    el.addEventListener("mousedown", onMinimapMouseDown);
    // The System theme stamps no attribute, so this is the only notice the
    // page gets that its colours have changed under it.
    var dark = window.matchMedia("(prefers-color-scheme: dark)");
    if (dark.addEventListener) dark.addEventListener("change", scheduleMinimapPaint);
    else if (dark.addListener) dark.addListener(scheduleMinimapPaint);
  }

  function attachSidebarListeners() {
    // Before the resizer check: the outline follows the reader whether or not
    // the panel can be dragged.
    window.addEventListener("scroll", schedulePositionSync, { passive: true });
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

  // The rendered diff carries each block's older version as inert <template>
  // content, so that nothing walks into it: not the outline, not find, not the
  // offsets comments are anchored against, and above all not mermaid, which
  // marks what it has drawn and would leave a diagram drawn at zero height
  // inside a closed fold broken for good. It is hydrated the first time a
  // reader opens one. Capture phase: toggle does not bubble.
  function attachRenderedDiffListeners() {
    document.addEventListener("toggle", function (event) {
      var details = event.target;
      if (!details || !details.classList) return;
      if (!details.classList.contains("mdview-rdiff-old") || !details.open) return;
      var template = details.querySelector("template");
      var body = details.querySelector(".mdview-rdiff-old-body");
      if (!template || !body) return;
      body.appendChild(template.content);
      details.removeChild(template);
      invalidateTextIndex();
      renderMath(body);
      renderDiagrams().then(enhanceZoomables).then(scheduleMinimapPaint);
    }, true);
  }

  document.addEventListener("DOMContentLoaded", function () {
    attachSidebarListeners();
    attachMinimapListeners();
    attachFindListeners();
    attachCommentListeners();
    attachRenderedDiffListeners();
    attachKeyListeners();
    window.mdviewRenderAll();
  });
})();
