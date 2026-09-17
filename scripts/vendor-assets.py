#!/usr/bin/env python3
"""Fetch KaTeX, Mermaid and Mermaid's ELK layout into crates/mdcore/assets/.

Run once; the results are committed. KaTeX's stylesheet references font files
by URL, which a CSP of `font-src data:` forbids and an offline app cannot
fetch, so every font reference is rewritten into an inlined data: URI.

Mermaid ships a browser bundle that is a plain script -- it ends by assigning
`globalThis.mermaid` -- so it drops straight into the nonce'd inline <script>
`page.rs` writes. ELK does not: `@mermaid-js/layout-elk` is ESM only, and its
entry lazily `import()`s a 1.6 MB chunk beside it. A page has no base URL an
import could resolve against (`loadHTMLString_baseURL` points at the user's
document directory), and jsDelivr's `+esm` leaves the dynamic import in place,
so the only way to get ELK offline is to bundle it here. esbuild's `iife`
format cannot code-split, which is exactly what we want: the dynamic import is
inlined and the result is one self-contained classic script.

That step needs npm, and only here. The output is committed, so building
MDView never asks for a JavaScript toolchain.

Two things about the bundle that are easy to worry about and should not be:
elkjs only constructs a real Worker when given a `workerUrl`, and it is not
given one, so it runs in-process and `default-src 'none'` is untouched. And
ELK lays out the flowchart family only -- sequence, gantt, pie and ER read
their own renderers and ignore `layout` entirely.
"""
import base64
import io
import pathlib
import re
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile

KATEX_VERSION = "0.16.11"
# 11.x, not 12: `@mermaid-js/layout-elk` declares `mermaid: ^11.0.2` as its
# peer, and 12's browser bundle is 5.4 MB against this one's 3.4 MB.
MERMAID_VERSION = "11.17.2"
ELK_VERSION = "0.1.9"
ESBUILD_VERSION = "0.25.10"
KATEX_ZIP = f"https://github.com/KaTeX/KaTeX/releases/download/v{KATEX_VERSION}/katex.zip"
MERMAID_JS = f"https://cdn.jsdelivr.net/npm/mermaid@{MERMAID_VERSION}/dist/mermaid.min.js"

ROOT = pathlib.Path(__file__).resolve().parent.parent
ASSETS = ROOT / "crates" / "mdcore" / "assets"
ASSETS.mkdir(parents=True, exist_ok=True)


def fetch(url: str) -> bytes:
    print(f"fetching {url}")
    with urllib.request.urlopen(url) as response:
        return response.read()


def inline_fonts(css: str, fonts: dict) -> str:
    """Rewrite url(fonts/X.woff2) into url(data:font/woff2;base64,...)."""
    def replace(match):
        name = pathlib.PurePosixPath(match.group(1).strip("'\"")).name
        blob = fonts.get(name)
        if blob is None:
            # Drop references we cannot inline; woff2 alone covers every
            # browser WebKit ships, so src fallbacks are expendable.
            return "url(about:blank)"
        encoded = base64.b64encode(blob).decode("ascii")
        return f"url(data:font/woff2;base64,{encoded})"

    return re.sub(r"url\(([^)]+)\)", replace, css)


def bundle_elk(out: pathlib.Path) -> None:
    """Bundle @mermaid-js/layout-elk into one classic script.

    The entry assigns the layout list to a global rather than exporting it:
    `diagrams.js` reads `window.mdviewElkLayouts` and hands it to
    `mermaid.registerLayoutLoaders`, and a global needs no `--global-name`
    indirection to unwrap on the other side.
    """
    if shutil.which("npx") is None:
        raise SystemExit(
            "npx not found. The ELK bundle needs npm, once, to build "
            f"{out.name}; the committed copy is what MDView actually ships."
        )

    with tempfile.TemporaryDirectory() as work:
        work = pathlib.Path(work)
        entry = work / "entry.mjs"
        entry.write_text(
            "import layouts from '@mermaid-js/layout-elk';\n"
            "globalThis.mdviewElkLayouts = layouts;\n",
            encoding="utf-8",
        )
        print(f"installing @mermaid-js/layout-elk@{ELK_VERSION}")
        subprocess.run(
            ["npm", "install", "--silent", "--no-package-lock", "--prefix", str(work),
             f"@mermaid-js/layout-elk@{ELK_VERSION}"],
            check=True,
        )
        print("bundling elk")
        subprocess.run(
            ["npx", "--yes", f"esbuild@{ESBUILD_VERSION}", str(entry),
             "--bundle", "--minify", "--format=iife", "--legal-comments=none",
             f"--outfile={out}"],
            check=True,
            cwd=work,
        )


def main() -> None:
    archive = zipfile.ZipFile(io.BytesIO(fetch(KATEX_ZIP)))
    names = archive.namelist()

    css_name = next(n for n in names if n.endswith("katex.min.css"))
    js_name = next(n for n in names if n.endswith("katex.min.js"))
    fonts = {
        pathlib.PurePosixPath(n).name: archive.read(n)
        for n in names
        if n.endswith(".woff2")
    }
    print(f"inlining {len(fonts)} katex fonts")

    css = archive.read(css_name).decode("utf-8")
    (ASSETS / "katex.css").write_text(inline_fonts(css, fonts), encoding="utf-8")
    (ASSETS / "katex.js").write_bytes(archive.read(js_name))
    (ASSETS / "mermaid.js").write_bytes(fetch(MERMAID_JS))
    bundle_elk(ASSETS / "mermaid-elk.js")

    for name in ("katex.css", "katex.js", "mermaid.js", "mermaid-elk.js"):
        size = (ASSETS / name).stat().st_size
        print(f"  {name}: {size // 1024} KB")


if __name__ == "__main__":
    main()
