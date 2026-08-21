#!/usr/bin/env python3
"""Fetch KaTeX and Mermaid into crates/mdcore/assets/.

Run once; the results are committed. KaTeX's stylesheet references font files
by URL, which a CSP of `font-src data:` forbids and an offline app cannot
fetch, so every font reference is rewritten into an inlined data: URI.
"""
import base64
import io
import pathlib
import re
import urllib.request
import zipfile

KATEX_VERSION = "0.16.11"
MERMAID_VERSION = "10.9.1"
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

    for name in ("katex.css", "katex.js", "mermaid.js"):
        size = (ASSETS / name).stat().st_size
        print(f"  {name}: {size // 1024} KB")


if __name__ == "__main__":
    main()
