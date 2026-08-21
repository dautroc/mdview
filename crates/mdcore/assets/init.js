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

  function renderDiagrams() {
    if (typeof mermaid === "undefined") return;
    try {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "default",
      });
      mermaid.run({ querySelector: "pre.mermaid" });
    } catch (err) {
      /* leave the diagram source visible as text */
    }
  }

  // Called on first load and again after every live-reload body swap.
  window.mdviewRenderAll = function () {
    renderMath();
    renderDiagrams();
  };

  document.addEventListener("DOMContentLoaded", window.mdviewRenderAll);
})();
