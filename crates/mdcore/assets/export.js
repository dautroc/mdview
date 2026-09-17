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

  // The runtime itself lives in diagrams.js, shared with the other page
  // runtime and inlined only into a page that has a diagram to draw. Absent,
  // this resolves like a document with no diagrams in it -- which is exactly
  // what such a page is.
  function renderDiagrams() {
    return window.mdviewRenderDiagrams ? window.mdviewRenderDiagrams() : Promise.resolve();
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
