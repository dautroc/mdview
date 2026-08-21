use std::path::{Path, PathBuf};

/// A Markdown file loaded from disk, with everything needed to render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The path as given to `load`.
    pub path: PathBuf,
    /// Absolute directory containing the file. Becomes the web view's base URL
    /// so that relative image paths resolve.
    pub base_dir: PathBuf,
    /// File contents, lossily decoded if necessary.
    pub source: String,
    /// True when the bytes on disk were not valid UTF-8.
    pub lossy: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} has no parent directory")]
    NoParent { path: PathBuf },
}

impl Document {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| DocumentError::Read {
            path: path.to_path_buf(),
            source,
        })?;

        // Invalid UTF-8 is never fatal: show what we can and flag it.
        let (source, lossy) = match String::from_utf8(bytes) {
            Ok(text) => (text, false),
            Err(err) => (
                String::from_utf8_lossy(err.as_bytes()).into_owned(),
                true,
            ),
        };

        // Canonicalize first: `Path::new("note.md").parent()` is `Some("")`,
        // which would make relative images resolve against nothing.
        let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let base_dir = absolute
            .parent()
            .ok_or_else(|| DocumentError::NoParent {
                path: path.to_path_buf(),
            })?
            .to_path_buf();

        Ok(Document {
            path: path.to_path_buf(),
            base_dir,
            source,
            lossy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdcore-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_utf8_file_and_records_base_dir() {
        let dir = temp_dir();
        let path = dir.join("note.md");
        std::fs::write(&path, "# Hello").unwrap();

        let doc = Document::load(&path).unwrap();

        assert_eq!(doc.source, "# Hello");
        assert!(!doc.lossy);
        assert_eq!(doc.base_dir, std::fs::canonicalize(&dir).unwrap());
    }

    #[test]
    fn invalid_utf8_decodes_lossily_and_sets_flag() {
        let dir = temp_dir();
        let path = dir.join("bad.md");
        std::fs::write(&path, [b'#', b' ', 0xff, 0xfe, b'!']).unwrap();

        let doc = Document::load(&path).unwrap();

        assert!(doc.lossy, "invalid UTF-8 must set the lossy flag, not fail");
        assert!(doc.source.starts_with("# "));
        assert!(doc.source.contains('\u{fffd}'));
    }

    #[test]
    fn missing_file_is_a_read_error() {
        let err = Document::load("/nonexistent/nope.md").unwrap_err();
        assert!(matches!(err, DocumentError::Read { .. }));
    }
}
