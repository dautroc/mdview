//! Pure Markdown link extraction, resolution, and workspace graph construction.
//!
//! This module deliberately performs no network access. Paths are resolved both
//! lexically and canonically so a link cannot leave the supplied workspace via
//! `..` components or symlinks.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::ops::Range;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Link,
    Image,
    Autolink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub slug: String,
    pub source: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingReason {
    InvalidPercentEncoding,
    InvalidPath,
    NotFound,
    NotIndexed,
    HeadingNotFound,
    UnsupportedScheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedLink {
    Document {
        path: PathBuf,
    },
    Heading {
        path: PathBuf,
        heading: Heading,
    },
    External {
        url: String,
    },
    Asset {
        path: PathBuf,
    },
    Missing {
        path: Option<PathBuf>,
        fragment: Option<String>,
        reason: MissingReason,
    },
    OutsideWorkspace {
        path: PathBuf,
    },
}

impl ResolvedLink {
    pub fn document_path(&self) -> Option<&Path> {
        match self {
            Self::Document { path } | Self::Heading { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn is_broken(&self) -> bool {
        matches!(self, Self::Missing { .. } | Self::OutsideWorkspace { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLink {
    pub source: Range<usize>,
    pub raw_destination: String,
    pub fragment: Option<String>,
    pub kind: LinkKind,
    pub resolved: ResolvedLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractedLink {
    source: Range<usize>,
    destination: String,
    kind: LinkKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub path: PathBuf,
    pub headings: Vec<Heading>,
    pub links: Vec<DocumentLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub source_path: PathBuf,
    pub link: DocumentLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPreview {
    pub title: String,
    pub markdown: String,
    pub truncated: bool,
}

/// A deterministic workspace graph. Document keys and resolved local targets
/// are canonical absolute paths whenever those files exist.
#[derive(Debug, Clone)]
pub struct LinkGraph {
    workspace_root: PathBuf,
    documents: BTreeMap<PathBuf, String>,
    metadata: BTreeMap<PathBuf, DocumentMetadata>,
    incoming: BTreeMap<PathBuf, Vec<Backlink>>,
}

impl LinkGraph {
    /// Build a bounded graph for one standalone document and the Markdown files
    /// it links to directly. This provides previews without treating the whole
    /// parent directory as an implicit workspace.
    pub fn for_document(path: impl AsRef<Path>, max_file_bytes: u64) -> io::Result<Self> {
        Self::for_document_with_root(path.as_ref(), Path::new("/"), None, max_file_bytes)
    }

    /// Build a bounded direct-target graph without allowing links to leave an
    /// already-open workspace or reach files excluded by workspace discovery
    /// while its full graph is still being indexed.
    pub fn for_document_in_workspace<I, P>(
        path: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
        workspace_documents: I,
        max_file_bytes: u64,
    ) -> io::Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let workspace_root = fs::canonicalize(workspace_root)?;
        let allowed = workspace_documents
            .into_iter()
            .filter_map(|path| fs::canonicalize(path).ok())
            .filter(|path| path.starts_with(&workspace_root))
            .collect::<BTreeSet<_>>();
        Self::for_document_with_root(
            path.as_ref(),
            &workspace_root,
            Some(&allowed),
            max_file_bytes,
        )
    }

    fn for_document_with_root(
        path: &Path,
        workspace_root: &Path,
        allowed: Option<&BTreeSet<PathBuf>>,
        max_file_bytes: u64,
    ) -> io::Result<Self> {
        let workspace_root = fs::canonicalize(workspace_root)?;
        let source_path = fs::canonicalize(path)?;
        if !source_path.starts_with(&workspace_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document is outside the workspace",
            ));
        }
        let source = read_bounded(&source_path, max_file_bytes)?;
        let mut documents = BTreeMap::new();
        documents.insert(source_path.clone(), source.clone());
        for link in extract_links(&source) {
            if is_external(&link.destination) {
                continue;
            }
            let (path_part, _) = split_fragment(&link.destination);
            if has_scheme(path_part) {
                continue;
            }
            let Ok(decoded) = percent_decode(path_part.split('?').next().unwrap_or(path_part))
            else {
                continue;
            };
            if decoded.is_empty() {
                continue;
            }
            let requested = source_path
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(decoded);
            let Ok(target) = fs::canonicalize(requested) else {
                continue;
            };
            if !target.starts_with(&workspace_root)
                || allowed.is_some_and(|allowed| !allowed.contains(&target))
                || !is_markdown(&target)
                || documents.contains_key(&target)
            {
                continue;
            }
            if let Ok(target_source) = read_bounded(&target, max_file_bytes) {
                documents.insert(target, target_source);
            }
        }
        Self::build(workspace_root, documents)
    }

    pub fn build<I, P, S>(workspace_root: impl AsRef<Path>, documents: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = (P, S)>,
        P: AsRef<Path>,
        S: Into<String>,
    {
        let workspace_root = fs::canonicalize(workspace_root)?;
        let mut graph = Self {
            workspace_root,
            documents: BTreeMap::new(),
            metadata: BTreeMap::new(),
            incoming: BTreeMap::new(),
        };
        for (path, source) in documents {
            let path = graph.document_key(path.as_ref());
            graph.documents.insert(path, source.into());
        }
        graph.rebuild();
        Ok(graph)
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn documents(&self) -> impl Iterator<Item = &DocumentMetadata> {
        self.metadata.values()
    }

    pub fn document(&self, path: impl AsRef<Path>) -> Option<&DocumentMetadata> {
        self.metadata.get(&self.document_key(path.as_ref()))
    }

    pub fn outgoing(&self, path: impl AsRef<Path>) -> &[DocumentLink] {
        self.document(path)
            .map(|document| document.links.as_slice())
            .unwrap_or(&[])
    }

    pub fn incoming(&self, path: impl AsRef<Path>) -> &[Backlink] {
        self.incoming
            .get(&self.document_key(path.as_ref()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn backlinks(&self, path: impl AsRef<Path>) -> &[Backlink] {
        self.incoming(path)
    }

    pub fn broken_outgoing(&self, path: impl AsRef<Path>) -> impl Iterator<Item = &DocumentLink> {
        self.outgoing(path)
            .iter()
            .filter(|link| link.resolved.is_broken())
    }

    /// A bounded Markdown slice for a local link preview. Heading links include
    /// that heading's section; document links begin at the document start.
    pub fn preview(&self, resolved: &ResolvedLink, max_bytes: usize) -> Option<LinkPreview> {
        let path = resolved.document_path()?;
        let source = self.documents.get(path)?;
        let metadata = self.metadata.get(path)?;
        let (title, start, section_end) = match resolved {
            ResolvedLink::Heading { heading, .. } => {
                let end = metadata
                    .headings
                    .iter()
                    .find(|candidate| {
                        candidate.source.start > heading.source.start
                            && candidate.level <= heading.level
                    })
                    .map(|candidate| candidate.source.start)
                    .unwrap_or(source.len());
                (heading.text.clone(), heading.source.start, end)
            }
            ResolvedLink::Document { .. } => {
                let title = metadata
                    .headings
                    .first()
                    .map(|heading| heading.text.clone())
                    .or_else(|| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| "Document".to_string());
                (title, 0, source.len())
            }
            _ => return None,
        };
        let limit = start.saturating_add(max_bytes).min(section_end);
        let end = floor_char_boundary(source, limit);
        Some(LinkPreview {
            title,
            markdown: source[start..end].to_string(),
            truncated: end < section_end,
        })
    }

    /// Insert or replace one document and refresh all dependent resolutions.
    /// Re-resolving the graph is intentional: changing a target's headings can
    /// change links in otherwise untouched source documents.
    pub fn upsert(&mut self, path: impl AsRef<Path>, source: impl Into<String>) {
        let key = self.document_key(path.as_ref());
        self.documents.insert(key, source.into());
        self.rebuild();
    }

    /// Remove a document and refresh backlinks and links that formerly targeted
    /// it. Returns whether a document was present.
    pub fn remove(&mut self, path: impl AsRef<Path>) -> bool {
        let key = self.document_key(path.as_ref());
        let removed = self.documents.remove(&key).is_some();
        if removed {
            self.rebuild();
        }
        removed
    }

    fn document_key(&self, path: &Path) -> PathBuf {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };
        fs::canonicalize(&candidate).unwrap_or(candidate)
    }

    fn rebuild(&mut self) {
        let heading_index = self
            .documents
            .iter()
            .map(|(path, source)| (path.clone(), headings(source)))
            .collect::<BTreeMap<_, _>>();
        let document_paths = self.documents.keys().cloned().collect::<BTreeSet<_>>();

        self.metadata = self
            .documents
            .iter()
            .map(|(path, source)| {
                let links = resolve_links(
                    source,
                    path,
                    &self.workspace_root,
                    &document_paths,
                    &heading_index,
                );
                (
                    path.clone(),
                    DocumentMetadata {
                        path: path.clone(),
                        headings: heading_index.get(path).cloned().unwrap_or_default(),
                        links,
                    },
                )
            })
            .collect();

        self.incoming.clear();
        for (source_path, document) in &self.metadata {
            for link in &document.links {
                if let Some(target) = link.resolved.document_path() {
                    self.incoming
                        .entry(target.to_path_buf())
                        .or_default()
                        .push(Backlink {
                            source_path: source_path.clone(),
                            link: link.clone(),
                        });
                }
            }
        }
        for backlinks in self.incoming.values_mut() {
            backlinks.sort_by(|left, right| {
                left.source_path
                    .cmp(&right.source_path)
                    .then(left.link.source.start.cmp(&right.link.source.start))
            });
        }
    }
}

/// Analyze one document against a supplied set of workspace documents.
/// Relative document-set paths are interpreted from `workspace_root`.
pub fn analyze_document<I, P>(
    markdown: &str,
    source_path: impl AsRef<Path>,
    workspace_root: impl AsRef<Path>,
    documents: I,
) -> io::Result<DocumentMetadata>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let workspace_root = fs::canonicalize(workspace_root)?;
    let source_path = absolute_key(&workspace_root, source_path.as_ref());
    let document_paths = documents
        .into_iter()
        .map(|path| absolute_key(&workspace_root, path.as_ref()))
        .collect::<BTreeSet<_>>();
    let own_headings = headings(markdown);
    let mut heading_index = BTreeMap::new();
    heading_index.insert(source_path.clone(), own_headings.clone());
    let links = resolve_links(
        markdown,
        &source_path,
        &workspace_root,
        &document_paths,
        &heading_index,
    );
    Ok(DocumentMetadata {
        path: source_path,
        headings: own_headings,
        links,
    })
}

/// Extract heading text and deterministic GitHub-like slugs. Duplicate slugs
/// receive `-1`, `-2`, ... suffixes in source order.
pub fn headings(markdown: &str) -> Vec<Heading> {
    let body = crate::frontmatter::strip(markdown);
    let body_offset = markdown.len() - body.len();
    let mut result = Vec::new();
    let mut current: Option<(u8, usize, String)> = None;
    let mut slug_counts = BTreeMap::<String, usize>::new();

    for (event, range) in Parser::new_ext(body, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((heading_level(level), range.start, String::new()));
            }
            Event::Text(text) | Event::Code(text) | Event::InlineMath(text) => {
                if let Some((_, _, value)) = current.as_mut() {
                    value.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, value)) = current.as_mut() {
                    value.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, start, text)) = current.take() {
                    let base = github_slug(&text);
                    let count = slug_counts.entry(base.clone()).or_default();
                    let slug = if *count == 0 {
                        base
                    } else {
                        format!("{base}-{count}")
                    };
                    *count += 1;
                    result.push(Heading {
                        level,
                        text,
                        slug,
                        source: body_offset + start..body_offset + range.end,
                    });
                }
            }
            _ => {}
        }
    }
    result
}

/// Lowercase text, remove punctuation other than `_` and `-`, and replace each
/// whitespace character with `-`. Duplicate handling belongs to [`headings`].
pub fn github_slug(text: &str) -> String {
    let mut slug = String::new();
    for character in text.trim().chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' || character == '-' {
            slug.push(character);
        } else if character.is_whitespace() {
            slug.push('-');
        }
    }
    if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    }
}

fn read_bounded(path: &Path, max_file_bytes: u64) -> io::Result<String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let opened = options.open(path)?;
    let mut bytes = Vec::new();
    opened
        .take(max_file_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_file_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document exceeds link preview limit",
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn resolve_links(
    markdown: &str,
    source_path: &Path,
    workspace_root: &Path,
    documents: &BTreeSet<PathBuf>,
    heading_index: &BTreeMap<PathBuf, Vec<Heading>>,
) -> Vec<DocumentLink> {
    extract_links(markdown)
        .into_iter()
        .map(|link| {
            let (path_part, raw_fragment) = split_fragment(&link.destination);
            let fragment = match raw_fragment {
                Some(value) => percent_decode(value).ok(),
                None => None,
            };
            let resolved = resolve_destination(
                &link.destination,
                path_part,
                raw_fragment,
                source_path,
                workspace_root,
                documents,
                heading_index,
            );
            DocumentLink {
                source: link.source,
                raw_destination: link.destination,
                fragment,
                kind: link.kind,
                resolved,
            }
        })
        .collect()
}

fn resolve_destination(
    raw: &str,
    path_part: &str,
    raw_fragment: Option<&str>,
    source_path: &Path,
    workspace_root: &Path,
    documents: &BTreeSet<PathBuf>,
    heading_index: &BTreeMap<PathBuf, Vec<Heading>>,
) -> ResolvedLink {
    if is_external(raw) {
        return ResolvedLink::External {
            url: raw.to_owned(),
        };
    }
    if has_scheme(path_part) {
        return ResolvedLink::Missing {
            path: None,
            fragment: raw_fragment.map(str::to_owned),
            reason: MissingReason::UnsupportedScheme,
        };
    }

    let decoded_path = match percent_decode(path_part.split('?').next().unwrap_or(path_part)) {
        Ok(path) => path,
        Err(()) => {
            return ResolvedLink::Missing {
                path: None,
                fragment: raw_fragment.map(str::to_owned),
                reason: MissingReason::InvalidPercentEncoding,
            };
        }
    };
    let fragment = match raw_fragment.map(percent_decode).transpose() {
        Ok(fragment) => fragment,
        Err(()) => {
            return ResolvedLink::Missing {
                path: None,
                fragment: raw_fragment.map(str::to_owned),
                reason: MissingReason::InvalidPercentEncoding,
            };
        }
    };

    let candidate = if decoded_path.is_empty() {
        source_path.to_path_buf()
    } else {
        match lexical_workspace_path(workspace_root, source_path, Path::new(&decoded_path)) {
            Ok(path) => path,
            Err(path) => return ResolvedLink::OutsideWorkspace { path },
        }
    };

    let canonical = match fs::canonicalize(&candidate) {
        Ok(path) => {
            if !path.starts_with(workspace_root) {
                return ResolvedLink::OutsideWorkspace { path };
            }
            path
        }
        Err(_) => {
            return ResolvedLink::Missing {
                path: Some(candidate),
                fragment,
                reason: MissingReason::NotFound,
            };
        }
    };

    if is_markdown(&canonical) {
        if !documents.contains(&canonical) {
            return ResolvedLink::Missing {
                path: Some(canonical),
                fragment,
                reason: MissingReason::NotIndexed,
            };
        }
        if let Some(fragment) = fragment.filter(|value| !value.is_empty()) {
            if let Some(heading) = heading_index
                .get(&canonical)
                .and_then(|items| items.iter().find(|heading| heading.slug == fragment))
            {
                return ResolvedLink::Heading {
                    path: canonical,
                    heading: heading.clone(),
                };
            }
            return ResolvedLink::Missing {
                path: Some(canonical),
                fragment: Some(fragment),
                reason: MissingReason::HeadingNotFound,
            };
        }
        ResolvedLink::Document { path: canonical }
    } else if canonical.is_file() {
        ResolvedLink::Asset { path: canonical }
    } else {
        ResolvedLink::Missing {
            path: Some(canonical),
            fragment,
            reason: MissingReason::InvalidPath,
        }
    }
}

fn extract_links(markdown: &str) -> Vec<ExtractedLink> {
    let body = crate::frontmatter::strip(markdown);
    let body_offset = markdown.len() - body.len();
    let mut links = Vec::new();
    let mut open: Option<(LinkKind, String, usize)> = None;

    for (event, range) in Parser::new_ext(body, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let kind = if matches!(link_type, LinkType::Autolink | LinkType::Email) {
                    LinkKind::Autolink
                } else {
                    LinkKind::Link
                };
                open = Some((kind, dest_url.into_string(), range.start));
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                open = Some((LinkKind::Image, dest_url.into_string(), range.start));
            }
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                if let Some((kind, destination, start)) = open.take() {
                    links.push(ExtractedLink {
                        source: start..range.end,
                        destination,
                        kind,
                    });
                }
            }
            Event::Text(text) if open.is_none() => {
                extract_bare_urls(body, &text, range, &mut links);
            }
            _ => {}
        }
    }
    for link in &mut links {
        link.source = body_offset + link.source.start..body_offset + link.source.end;
    }
    links.sort_by_key(|link| link.source.start);
    links
}

fn extract_bare_urls(
    markdown: &str,
    text: &str,
    event_range: Range<usize>,
    links: &mut Vec<ExtractedLink>,
) {
    let source = &markdown[event_range.clone()];
    let Some(text_offset) = source.find(text) else {
        return;
    };
    let base = event_range.start + text_offset;
    let mut cursor = 0;
    while cursor < text.len() {
        let rest = &text[cursor..];
        let Some(relative) = ["https://", "http://", "mailto:"]
            .iter()
            .filter_map(|prefix| rest.find(prefix))
            .min()
        else {
            break;
        };
        let start = cursor + relative;
        if start > 0 && is_url_character(text[..start].chars().next_back().unwrap_or(' ')) {
            cursor = start + 1;
            continue;
        }
        let mut end = start;
        for (offset, character) in text[start..].char_indices() {
            if character.is_whitespace() || matches!(character, '<' | '>' | '"') {
                break;
            }
            end = start + offset + character.len_utf8();
        }
        while end > start
            && text[..end]
                .chars()
                .next_back()
                .is_some_and(|character| matches!(character, '.' | ',' | ';' | ':' | '!' | '?'))
        {
            end -= text[..end].chars().next_back().unwrap().len_utf8();
        }
        if end > start {
            links.push(ExtractedLink {
                source: base + start..base + end,
                destination: text[start..end].to_owned(),
                kind: LinkKind::Autolink,
            });
        }
        cursor = end.max(start + 1);
    }
}

fn split_fragment(destination: &str) -> (&str, Option<&str>) {
    match destination.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (destination, None),
    }
}

fn percent_decode(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let high = hex(bytes[index + 1]).ok_or(())?;
            let low = hex(bytes[index + 2]).ok_or(())?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn lexical_workspace_path(
    root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<PathBuf, PathBuf> {
    let mut parts = if destination.is_absolute() {
        Vec::new()
    } else {
        source
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .map(|relative| {
                relative
                    .components()
                    .filter_map(|component| match component {
                        Component::Normal(value) => Some(value.to_owned()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    for component in destination.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_owned()),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(root.join(destination));
                }
            }
            Component::CurDir | Component::RootDir => {}
            Component::Prefix(_) => return Err(destination.to_path_buf()),
        }
    }
    Ok(parts
        .into_iter()
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

fn absolute_key(root: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    fs::canonicalize(&path).unwrap_or(path)
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md")
                || extension.eq_ignore_ascii_case("markdown")
                || extension.eq_ignore_ascii_case("mdown")
        })
}

fn is_external(destination: &str) -> bool {
    let lower = destination.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

fn has_scheme(destination: &str) -> bool {
    let Some(colon) = destination.find(':') else {
        return false;
    };
    let scheme = &destination[..colon];
    !scheme.is_empty()
        && scheme.chars().enumerate().all(|(index, character)| {
            if index == 0 {
                character.is_ascii_alphabetic()
            } else {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            }
        })
}

fn is_url_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/' | ':')
}

fn heading_level(level: pulldown_cmark::HeadingLevel) -> u8 {
    match level {
        pulldown_cmark::HeadingLevel::H1 => 1,
        pulldown_cmark::HeadingLevel::H2 => 2,
        pulldown_cmark::HeadingLevel::H3 => 3,
        pulldown_cmark::HeadingLevel::H4 => 4,
        pulldown_cmark::HeadingLevel::H5 => 5,
        pulldown_cmark::HeadingLevel::H6 => 6,
    }
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempWorkspace(PathBuf);

    impl TempWorkspace {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("mdview-links-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn graph(workspace: &TempWorkspace, files: &[(&str, &str)]) -> LinkGraph {
        for (path, source) in files {
            workspace.write(path, source);
        }
        LinkGraph::build(
            &workspace.0,
            files.iter().map(|(path, source)| (*path, *source)),
        )
        .unwrap()
    }

    #[test]
    fn resolves_relative_and_parent_document_links() {
        let workspace = TempWorkspace::new();
        let graph = graph(
            &workspace,
            &[
                ("guide/start.md", "[next](next.md) [readme](../README.md)"),
                ("guide/next.md", "# Next"),
                ("README.md", "# Home"),
            ],
        );
        let links = graph.outgoing("guide/start.md");
        assert_eq!(links.len(), 2);
        assert!(
            matches!(&links[0].resolved, ResolvedLink::Document { path } if path.ends_with("guide/next.md"))
        );
        assert!(
            matches!(&links[1].resolved, ResolvedLink::Document { path } if path.ends_with("README.md"))
        );
    }

    #[test]
    fn resolves_same_and_other_document_fragments_and_duplicate_headings() {
        let workspace = TempWorkspace::new();
        let graph = graph(
            &workspace,
            &[
                (
                    "a.md",
                    "# Repeat\n# Repeat\n[first](#repeat) [second](#repeat-1) [other](b.md#target)",
                ),
                ("b.md", "## Target"),
            ],
        );
        let document = graph.document("a.md").unwrap();
        assert_eq!(
            document
                .headings
                .iter()
                .map(|heading| heading.slug.as_str())
                .collect::<Vec<_>>(),
            vec!["repeat", "repeat-1"]
        );
        assert!(
            matches!(&document.links[0].resolved, ResolvedLink::Heading { heading, .. } if heading.slug == "repeat")
        );
        assert!(
            matches!(&document.links[1].resolved, ResolvedLink::Heading { heading, .. } if heading.slug == "repeat-1")
        );
        assert!(
            matches!(&document.links[2].resolved, ResolvedLink::Heading { path, heading } if path.ends_with("b.md") && heading.slug == "target")
        );
    }

    #[test]
    fn decodes_url_paths_and_fragments_but_rejects_invalid_encoding() {
        let workspace = TempWorkspace::new();
        let graph = graph(
            &workspace,
            &[
                (
                    "index.md",
                    "[ok](docs/a%20file.md#caf%C3%A9) [bad](docs/%ZZ.md) [utf8](docs/%FF.md)",
                ),
                ("docs/a file.md", "# Café"),
            ],
        );
        let links = graph.outgoing("index.md");
        assert_eq!(links[0].fragment.as_deref(), Some("café"));
        assert!(
            matches!(&links[0].resolved, ResolvedLink::Heading { path, .. } if path.ends_with("docs/a file.md"))
        );
        assert!(matches!(
            &links[1].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::InvalidPercentEncoding,
                ..
            }
        ));
        assert!(matches!(
            &links[2].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::InvalidPercentEncoding,
                ..
            }
        ));
    }

    #[test]
    fn extracts_inline_reference_image_and_external_links_distinctly() {
        let workspace = TempWorkspace::new();
        workspace.write("pic.png", "png");
        let graph = graph(
            &workspace,
            &[(
                "index.md",
                "[inline](other.md) [reference][ref] ![image](pic.png) <https://example.com/a> https://example.org/raw\n\n[ref]: other.md",
            ), ("other.md", "# Other")],
        );
        let links = graph.outgoing("index.md");
        assert_eq!(links.len(), 5);
        assert_eq!(
            links.iter().map(|link| link.kind).collect::<Vec<_>>(),
            vec![
                LinkKind::Link,
                LinkKind::Link,
                LinkKind::Image,
                LinkKind::Autolink,
                LinkKind::Autolink
            ]
        );
        assert!(matches!(links[2].resolved, ResolvedLink::Asset { .. }));
        assert!(matches!(links[3].resolved, ResolvedLink::External { .. }));
        assert!(matches!(links[4].resolved, ResolvedLink::External { .. }));
        for link in links {
            assert!(!link.source.is_empty());
        }
    }

    #[test]
    fn reports_missing_files_headings_and_extensionless_targets() {
        let workspace = TempWorkspace::new();
        let graph = graph(
            &workspace,
            &[
                (
                    "index.md",
                    "[file](gone.md) [heading](target.md#gone) [no guessing](target)",
                ),
                ("target.md", "# Present"),
            ],
        );
        let links = graph.outgoing("index.md");
        assert!(matches!(
            &links[0].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::NotFound,
                ..
            }
        ));
        assert!(matches!(
            &links[1].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::HeadingNotFound,
                ..
            }
        ));
        assert!(matches!(
            &links[2].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::NotFound,
                ..
            }
        ));
        assert_eq!(graph.broken_outgoing("index.md").count(), 3);
    }

    #[test]
    fn rejects_lexical_root_escape() {
        let parent = TempWorkspace::new();
        let root = parent.0.join("root");
        fs::create_dir(&root).unwrap();
        fs::write(parent.0.join("outside.md"), "# Outside").unwrap();
        fs::write(root.join("index.md"), "[out](../outside.md)").unwrap();
        let graph = LinkGraph::build(&root, [("index.md", "[out](../outside.md)")]).unwrap();
        assert!(matches!(
            graph.outgoing("index.md")[0].resolved,
            ResolvedLink::OutsideWorkspace { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let parent = TempWorkspace::new();
        let root = parent.0.join("root");
        fs::create_dir(&root).unwrap();
        fs::write(parent.0.join("outside.md"), "# Outside").unwrap();
        fs::write(root.join("index.md"), "[out](escape.md)").unwrap();
        symlink(parent.0.join("outside.md"), root.join("escape.md")).unwrap();
        let graph = LinkGraph::build(&root, [("index.md", "[out](escape.md)")]).unwrap();
        assert!(matches!(
            graph.outgoing("index.md")[0].resolved,
            ResolvedLink::OutsideWorkspace { .. }
        ));
    }

    #[test]
    fn incremental_updates_match_rebuild_and_refresh_backlinks() {
        let workspace = TempWorkspace::new();
        workspace.write("a.md", "[b](b.md)");
        workspace.write("b.md", "# B");
        workspace.write("c.md", "# C");
        let mut incremental =
            LinkGraph::build(&workspace.0, [("a.md", "[b](b.md)"), ("b.md", "# B")]).unwrap();
        assert_eq!(incremental.backlinks("b.md").len(), 1);

        incremental.upsert("a.md", "[c](c.md)");
        incremental.upsert("c.md", "# C");
        assert!(incremental.backlinks("b.md").is_empty());
        assert_eq!(incremental.backlinks("c.md").len(), 1);

        let rebuilt = LinkGraph::build(
            &workspace.0,
            [("a.md", "[c](c.md)"), ("b.md", "# B"), ("c.md", "# C")],
        )
        .unwrap();
        assert_eq!(incremental.metadata, rebuilt.metadata);
        assert_eq!(incremental.incoming, rebuilt.incoming);

        assert!(incremental.remove("c.md"));
        assert!(incremental.backlinks("c.md").is_empty());
        assert!(matches!(
            incremental.outgoing("a.md")[0].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::NotIndexed,
                ..
            }
        ));
    }

    #[test]
    fn changing_target_headings_re_resolves_untouched_sources() {
        let workspace = TempWorkspace::new();
        let mut graph = graph(
            &workspace,
            &[("a.md", "[target](b.md#old)"), ("b.md", "# Old")],
        );
        assert!(matches!(
            graph.outgoing("a.md")[0].resolved,
            ResolvedLink::Heading { .. }
        ));
        graph.upsert("b.md", "# New");
        assert!(matches!(
            graph.outgoing("a.md")[0].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::HeadingNotFound,
                ..
            }
        ));
    }

    #[test]
    fn frontmatter_is_excluded_without_losing_source_offsets() {
        let workspace = TempWorkspace::new();
        let source = "---\ntitle: Hidden\n---\n\n# Visible\n\n[target](b.md#there)";
        let graph = graph(
            &workspace,
            &[("a.md", source), ("b.md", "# There\n\nTarget body.")],
        );
        let document = graph.document("a.md").unwrap();
        assert_eq!(document.headings.len(), 1);
        assert_eq!(document.headings[0].slug, "visible");
        assert_eq!(
            &source[document.links[0].source.clone()],
            "[target](b.md#there)"
        );
        assert!(matches!(
            document.links[0].resolved,
            ResolvedLink::Heading { .. }
        ));
    }

    #[test]
    fn preview_is_bounded_to_the_target_section_and_unicode_safe() {
        let workspace = TempWorkspace::new();
        let graph = graph(
            &workspace,
            &[
                ("a.md", "[target](b.md#details)"),
                ("b.md", "# Intro\n\nSkip.\n\n## Details\n\nλambda words that continue.\n\n## Next\n\nNot included."),
            ],
        );
        let preview = graph
            .preview(&graph.outgoing("a.md")[0].resolved, 36)
            .unwrap();
        assert_eq!(preview.title, "Details");
        assert!(preview.markdown.starts_with("## Details"));
        assert!(!preview.markdown.contains("Not included"));
        assert!(preview.truncated);
        assert!(std::str::from_utf8(preview.markdown.as_bytes()).is_ok());
    }

    #[test]
    fn standalone_graph_loads_only_direct_markdown_targets() {
        let workspace = TempWorkspace::new();
        let source = workspace.write("a.md", "[target](b.md#there)");
        workspace.write("b.md", "# There\n\nPreview me.");
        workspace.write("unrelated.md", "# Not indexed");
        let graph = LinkGraph::for_document(&source, 1024).unwrap();
        assert!(matches!(
            graph.outgoing(&source)[0].resolved,
            ResolvedLink::Heading { .. }
        ));
        assert!(graph.document(workspace.0.join("unrelated.md")).is_none());
    }

    #[test]
    fn workspace_scoped_direct_graph_never_loads_a_parent_target() {
        let workspace = TempWorkspace::new();
        let source = workspace.write(
            "docs/a.md",
            "[inside](b.md) [excluded](private.md) [outside](../outside.md)",
        );
        workspace.write("docs/b.md", "# Inside");
        workspace.write("docs/private.md", "# Excluded");
        workspace.write("outside.md", "# Outside");
        let graph = LinkGraph::for_document_in_workspace(
            &source,
            workspace.0.join("docs"),
            [source.clone(), workspace.0.join("docs/b.md")],
            1024,
        )
        .unwrap();
        assert!(matches!(
            graph.outgoing(&source)[0].resolved,
            ResolvedLink::Document { .. }
        ));
        assert!(matches!(
            graph.outgoing(&source)[1].resolved,
            ResolvedLink::Missing {
                reason: MissingReason::NotIndexed,
                ..
            }
        ));
        assert!(matches!(
            graph.outgoing(&source)[2].resolved,
            ResolvedLink::OutsideWorkspace { .. }
        ));
        assert!(graph.document(workspace.0.join("docs/private.md")).is_none());
        assert!(graph.document(workspace.0.join("outside.md")).is_none());
    }
}
