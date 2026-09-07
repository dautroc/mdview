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
        node.textContent = tex;
      }
    }
  }

  function stashMermaidSources() {
    var nodes = document.querySelectorAll("pre.mermaid");
    for (var i = 0; i < nodes.length; i++) {
      if (!nodes[i].hasAttribute("data-mermaid-src")) {
        nodes[i].setAttribute("data-mermaid-src", nodes[i].textContent);
      }
    }
  }

  function effectiveTheme() {
    var stamped = document.documentElement.getAttribute("data-dark");
    if (stamped === "1") return "dark";
    if (stamped === "0") return "light";
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

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
          /* Leave invalid diagram source visible. */
        });
      }
      return Promise.resolve();
    } catch (err) {
      return Promise.resolve();
    }
  }

  var renderGeneration = 0;
  window.mdviewRenderState = {
    generation: 0,
    status: "pending",
    error: null,
  };

  window.mdviewRenderAll = function () {
    var generation = ++renderGeneration;
    window.mdviewRenderState = {
      generation: generation,
      status: "rendering",
      error: null,
    };

    try {
      renderMath();
      window.mdviewRenderPromise = renderDiagrams().then(function () {
        if (window.mdviewRenderState.generation === generation) {
          window.mdviewRenderState.status = "ready";
          document.documentElement.setAttribute("data-render-ready", String(generation));
        }
        return window.mdviewRenderState;
      }, function (error) {
        if (window.mdviewRenderState.generation === generation) {
          window.mdviewRenderState.status = "failed";
          window.mdviewRenderState.error = String(error || "Rendering failed");
        }
        return window.mdviewRenderState;
      });
    } catch (error) {
      window.mdviewRenderState.status = "failed";
      window.mdviewRenderState.error = String(error || "Rendering failed");
      window.mdviewRenderPromise = Promise.resolve(window.mdviewRenderState);
    }

    return window.mdviewRenderPromise;
  };

  document.addEventListener("DOMContentLoaded", function () {
    window.mdviewRenderAll();
  });
})();
