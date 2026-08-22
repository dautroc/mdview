//! Inlining local images as `data:` URIs.
//!
//! The app hands its HTML to WKWebView with `loadHTMLString`, and a page
//! loaded that way cannot read `file:` subresources however the path is
//! written -- WebKit sandboxes it, and only `loadFileURL` grants directory
//! access. So a document's own images never appeared. Embedding the bytes
//! sidesteps the sandbox entirely, and makes `--print-html` output genuinely
//! self-contained, which is what the crate already claims to produce.

use std::path::{Path, PathBuf};

/// Largest image to embed. Base64 costs a third on top, and the whole page is
/// held as one string, so a pathological file would balloon memory. Anything
/// bigger keeps its original `src` and simply does not render.
pub const MAX_INLINE_BYTES: u64 = 8 * 1024 * 1024;

/// Turn an image destination into a `data:` URI, or `None` to leave it alone.
///
/// Remote and already-inlined sources are left untouched; so is anything that
/// cannot be read, is too large, or escapes to a type we cannot label.
pub fn inline(dest: &str, base_dir: &Path) -> Option<String> {
    if is_remote(dest) {
        return None;
    }
    let path = resolve(dest, base_dir)?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_INLINE_BYTES {
        return None;
    }
    let mime = mime_for(&path)?;
    let bytes = std::fs::read(&path).ok()?;
    Some(format!("data:{mime};base64,{}", base64(&bytes)))
}

/// True for anything already addressable by the page: a URL with a scheme, a
/// protocol-relative URL, or a fragment.
fn is_remote(dest: &str) -> bool {
    let lower = dest.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("//")
        || lower.starts_with('#')
}

/// Resolve a Markdown destination against the document's directory.
///
/// Rejects nothing on the grounds of escaping the directory: a document may
/// legitimately reference `../shared/logo.png`, and the person opening it
/// already has read access to whatever they point at.
fn resolve(dest: &str, base_dir: &Path) -> Option<PathBuf> {
    // Strip any query or fragment, which Markdown allows on image paths.
    let cleaned = dest.split(['?', '#']).next()?.trim();
    if cleaned.is_empty() {
        return None;
    }
    let decoded = percent_decode(cleaned);
    // A `file:` URL is a local path once its scheme is gone.
    let decoded = decoded
        .strip_prefix("file://")
        .map(str::to_string)
        .unwrap_or(decoded);
    let path = Path::new(&decoded);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    })
}

/// Decode `%XX` escapes. Markdown writers percent-encode spaces routinely, and
/// the filesystem wants the real name.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Label the bytes by extension. Sniffing the content would be more robust,
/// but a wrong label renders nothing while a missing one is honest.
fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tif" | "tiff" => "image/tiff",
        "heic" => "image/heic",
        _ => return None,
    })
}

/// Standard base64 with padding. Written out rather than pulled in: it is
/// twenty lines, and the crate carries no dependency it does not need.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18 & 0x3f) as usize] as char);
        out.push(ALPHABET[(triple >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdcore-images-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The 1x1 transparent PNG, as bytes.
    fn png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89,
        ]
    }

    #[test]
    fn base64_matches_known_vectors() {
        // Padding is the easy thing to get wrong, so cover all three lengths.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn a_relative_image_becomes_a_data_uri() {
        let dir = tmp();
        std::fs::write(dir.join("shot.png"), png()).unwrap();
        let uri = inline("shot.png", &dir).expect("should inline");
        assert!(uri.starts_with("data:image/png;base64,iVBORw0KGgo"));
    }

    #[test]
    fn a_percent_encoded_name_still_resolves() {
        // Markdown writers escape spaces; the filesystem has the real name.
        let dir = tmp();
        std::fs::write(dir.join("my shot.png"), png()).unwrap();
        assert!(inline("my%20shot.png", &dir).is_some());
    }

    #[test]
    fn a_query_or_fragment_is_ignored() {
        let dir = tmp();
        std::fs::write(dir.join("q.png"), png()).unwrap();
        assert!(inline("q.png?v=2", &dir).is_some());
        assert!(inline("q.png#frag", &dir).is_some());
    }

    #[test]
    fn remote_and_inlined_sources_are_left_alone() {
        let dir = tmp();
        for dest in [
            "https://example.com/a.png",
            "http://example.com/a.png",
            "//example.com/a.png",
            "data:image/png;base64,AAAA",
        ] {
            assert_eq!(inline(dest, &dir), None, "{dest} must be left alone");
        }
    }

    #[test]
    fn a_missing_or_unlabelable_file_is_left_alone() {
        let dir = tmp();
        assert_eq!(inline("nope.png", &dir), None);
        std::fs::write(dir.join("thing.xyz"), b"x").unwrap();
        assert_eq!(inline("thing.xyz", &dir), None, "unknown type has no mime");
    }

    #[test]
    fn an_oversized_image_is_left_alone() {
        // Better a missing picture than a page that will not fit in memory.
        let dir = tmp();
        let big = dir.join("big.png");
        std::fs::write(&big, vec![0u8; (MAX_INLINE_BYTES + 1) as usize]).unwrap();
        assert_eq!(inline("big.png", &dir), None);
        std::fs::remove_file(&big).ok();
    }

    #[test]
    fn a_parent_relative_path_resolves() {
        // ../shared/logo.png is a normal way to lay documents out.
        let dir = tmp();
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("up.png"), png()).unwrap();
        assert!(inline("../up.png", &nested).is_some());
    }
}
