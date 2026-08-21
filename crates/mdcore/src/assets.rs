//! Compile-time embedded assets. Nothing here is read from disk at runtime,
//! which is what lets the app work with no network and no resource lookups.

pub const PAGE_CSS: &str = include_str!("../assets/page.css");
pub const INIT_JS: &str = include_str!("../assets/init.js");
pub const KATEX_CSS: &str = include_str!("../assets/katex.css");
pub const KATEX_JS: &str = include_str!("../assets/katex.js");
pub const MERMAID_JS: &str = include_str!("../assets/mermaid.js");
