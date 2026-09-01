//! The review file grammar: the on-disk form of a document's comments.
//!
//! Pure, like `state.rs` — no I/O, no AppKit. `store.rs` does the reading and
//! writing; everything that decides anything is here, where it is tested.
//!
//! The file is Markdown so a human can read it and Claude can edit it, but the
//! grammar is a line grammar first and Markdown second. The obvious shape — a
//! heading per comment, a blockquote for the quote, the note as prose — cannot
//! round-trip: this is a review tool, so notes contain `---`, `##` and
//! blockquotes as a matter of course, and a blockquote is not closed under its
//! own content.
//!
//! Fenced blocks are the one Markdown construct that is, because CommonMark
//! closes a fence only on a run of the same character at least as long as the
//! opener. Growing the fence past anything in the payload (`fence_for`) is
//! therefore the whole round-trip proof: no payload line can close its own
//! fence, so no payload byte can be read as structure.
//!
//! Everything outside the fences — the title, the path, the `## N.` headings,
//! any prose someone adds between records — is decorative. The parser skips it
//! and `serialize_review` regenerates it, so hand-editing the prose cannot
//! move a comment.

/// One comment: what was selected, where it was, and what was said about it.
///
/// `heading` is a 1-based ordinal into the document's headings — the nearest
/// one *preceding* the quote, or 0 for text above the first heading. Ordinals,
/// not ids: `buildOutline` in `init.js` is the only code that assigns heading
/// ids, and it runs only while the outline panel is showing, so an id is a
/// field that is frequently absent.
///
/// `nth` is the 0-based occurrence of `quote` within that heading's section,
/// counted at capture time. It is what tells two identical quotes apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub heading: usize,
    pub nth: usize,
    pub quote: String,
    pub note: String,
}

impl Comment {
    /// Normalizing constructor. Carriage returns and a trailing newline are
    /// stripped HERE rather than at serialize time, because "join the fence's
    /// lines with \n" is only the exact inverse of "emit the payload, then a
    /// newline" when the payload cannot itself end in one. Without this the
    /// round-trip identity is false for any payload ending in a newline.
    pub fn new(id: &str, heading: usize, nth: usize, quote: &str, note: &str) -> Self {
        Self {
            id: id.to_string(),
            heading,
            nth,
            quote: normalize(quote),
            note: normalize(note),
        }
    }
}

/// Strip carriage returns and any trailing newlines. See `Comment::new`.
fn normalize(text: &str) -> String {
    let text = text.replace('\r', "");
    text.trim_end_matches('\n').to_string()
}

/// The shortest fence that `payload` cannot close: one tilde longer than the
/// longest leading run in it, and never shorter than four.
///
/// Leading whitespace is stripped before counting rather than the three spaces
/// CommonMark actually allows, because being too conservative only ever makes
/// the fence longer, and being too permissive makes the file unparseable.
pub fn fence_for(payload: &str) -> String {
    let longest = payload
        .lines()
        .map(|line| line.trim_start().chars().take_while(|c| *c == '~').count())
        .max()
        .unwrap_or(0);
    "~".repeat(std::cmp::max(4, longest + 1))
}

/// The id no existing comment is using: the smallest positive integer free in
/// `existing`, in hex. Deterministic, so a test can pin it — the ids end up in
/// a file shared with Claude, and a random one would churn the diff on every
/// write.
pub fn fresh_id(existing: &[Comment]) -> String {
    for n in 1u64.. {
        let candidate = format!("{n:x}");
        if !existing.iter().any(|c| c.id == candidate) {
            return candidate;
        }
    }
    unreachable!("u64 exhausted")
}

/// Render the review file. `headings` is the document's heading text in
/// document order; a missing entry just costs a nicer label.
pub fn serialize_review(doc_path: &str, headings: &[String], comments: &[Comment]) -> String {
    let name = doc_path.rsplit('/').next().unwrap_or(doc_path);
    let mut out = format!("# Review — {name}\n{doc_path}\n");
    for (index, comment) in comments.iter().enumerate() {
        let label = match comment.heading.checked_sub(1).and_then(|i| headings.get(i)) {
            Some(text) => text.clone(),
            None => "(before the first heading)".to_string(),
        };
        out.push_str(&format!("\n## {}. {}\n\n", index + 1, label));
        let fence = fence_for(&comment.quote);
        out.push_str(&format!(
            "{fence} mdview-quote {} {} {}\n{}\n{fence}\n",
            comment.id, comment.heading, comment.nth, comment.quote
        ));
        if !comment.note.is_empty() {
            let fence = fence_for(&comment.note);
            out.push_str(&format!(
                "\n{fence} mdview-note\n{}\n{fence}\n",
                comment.note
            ));
        }
    }
    out
}

/// What a fence opener announced: the block kind, and the run length that will
/// close it.
struct Opener {
    kind: String,
    info: Vec<String>,
    width: usize,
}

/// Read a fence opener out of a line, or `None` if the line is not one.
///
/// Three tildes is accepted even though four is emitted: three is CommonMark's
/// minimum, and a file that has been through another Markdown tool should
/// still parse.
fn opener(line: &str) -> Option<Opener> {
    let trimmed = line.trim_start();
    let width = trimmed.chars().take_while(|c| *c == '~').count();
    if width < 3 {
        return None;
    }
    let mut info = trimmed[width..].split_whitespace().map(str::to_string);
    let kind = info.next()?;
    Some(Opener {
        kind,
        info: info.collect(),
        width,
    })
}

/// Does this line close a fence `width` tildes wide?
fn closes(line: &str, width: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= width && trimmed.chars().all(|c| c == '~')
}

/// Parse a review file. Never fails: a malformed record is skipped and a
/// truncated file yields the records it could read, matching the contract
/// `parse_message` states in `state.rs`. Dropping the whole file because one
/// record is bad would lose comments a human still has on screen.
pub fn parse_review(text: &str) -> Vec<Comment> {
    let lines: Vec<&str> = text.lines().collect();
    let mut comments: Vec<Comment> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(open) = opener(lines[index]) else {
            index += 1;
            continue;
        };
        if open.kind != "mdview-quote" && open.kind != "mdview-note" {
            index += 1;
            continue;
        }
        // Consume the payload even for a record we are going to discard, or
        // its text would be re-read as structure on the next pass.
        let mut payload: Vec<&str> = Vec::new();
        index += 1;
        while index < lines.len() && !closes(lines[index], open.width) {
            payload.push(lines[index]);
            index += 1;
        }
        index += 1; // step over the closing fence, or off the end
        let body = payload.join("\n");

        if open.kind == "mdview-note" {
            // Attaches to the most recent quote that has not been given one,
            // so the decorative headings between them do not break the pair.
            if let Some(last) = comments.last_mut() {
                if last.note.is_empty() {
                    last.note = body;
                }
            }
            continue;
        }
        let [id, heading, nth] = open.info.as_slice() else {
            continue;
        };
        let (Ok(heading), Ok(nth)) = (heading.parse::<usize>(), nth.parse::<usize>()) else {
            continue;
        };
        comments.push(Comment::new(id, heading, nth, &body, ""));
    }
    comments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(quote: &str, note: &str) -> Comment {
        Comment::new("1", 2, 0, quote, note)
    }

    /// The whole point of the fenced grammar. Every string here is one a
    /// reviewer plausibly types — `---` and `##` because they are reviewing
    /// Markdown, backticks because they are quoting code — and every one of
    /// them is a delimiter in the obvious blockquote-and-heading design.
    #[test]
    fn serialize_then_parse_is_the_identity_for_every_awkward_payload() {
        let awkward = [
            "",
            "   ",
            "a: b: c",
            "## Heading",
            "> quoted",
            ">> nested",
            "---",
            "***",
            "~~~~~",
            "~~~~~~~~~~",
            "  ~~~~ mdview-quote 9 9 9",
            "```rust",
            "`inline` and ``double``",
            "line one\nline two\nline three",
            "trailing spaces   ",
            "emoji 🎯 and accents é",
            "# Review — notes.md",
        ];
        for quote in awkward {
            for note in awkward {
                let original = vec![c(quote, note)];
                let text = serialize_review("/tmp/notes.md", &[], &original);
                assert_eq!(
                    parse_review(&text),
                    original,
                    "round trip failed for quote {quote:?} note {note:?}"
                );
            }
        }
    }

    #[test]
    fn a_payload_containing_a_fence_gets_a_longer_one() {
        assert_eq!(fence_for("plain"), "~~~~");
        assert_eq!(fence_for("~~~~"), "~~~~~");
        assert_eq!(fence_for("~~~~~~~~"), "~~~~~~~~~");
        // Indented, because CommonMark closes on an indented fence too.
        assert_eq!(fence_for("   ~~~~~"), "~~~~~~");
        // A tilde run that is not at the start of a line closes nothing.
        assert_eq!(fence_for("a ~~~~~~~~"), "~~~~");
    }

    /// The file is shared with Claude, which will add prose around the records
    /// it is answering. That prose must survive being re-read.
    #[test]
    fn prose_a_human_added_between_records_is_skipped_not_lost() {
        let original = vec![
            Comment::new("1", 1, 0, "first quote", "first note"),
            Comment::new("2", 3, 1, "second quote", "second note"),
        ];
        let text = serialize_review("/tmp/notes.md", &[], &original);
        let annotated = format!(
            "{text}\n\n## Notes from Claude\n\nI fixed the first one. The second\nneeds a decision from you.\n"
        );
        assert_eq!(parse_review(&annotated), original);
    }

    /// A crash mid-write, or a file someone truncated by hand. Dropping every
    /// record because the last one is short would lose comments still on
    /// screen.
    #[test]
    fn a_truncated_file_yields_the_records_it_could_read() {
        let original = vec![
            Comment::new("1", 1, 0, "kept", "kept note"),
            Comment::new("2", 1, 0, "cut off", ""),
        ];
        let text = serialize_review("/tmp/notes.md", &[], &original);
        let cut = &text[..text.len() - 6];
        assert_eq!(parse_review(cut), original);
    }

    #[test]
    fn a_record_with_no_note_block_parses_as_an_empty_note() {
        let text = "~~~~ mdview-quote a 4 2\njust the quote\n~~~~\n";
        assert_eq!(
            parse_review(text),
            vec![Comment::new("a", 4, 2, "just the quote", "")]
        );
    }

    /// The headings are regenerated on every write, so editing one by hand
    /// must not be able to re-anchor the comment under it.
    #[test]
    fn a_changed_decorative_heading_does_not_move_a_comment() {
        let original = vec![Comment::new("1", 7, 0, "quote", "note")];
        let text = serialize_review("/tmp/notes.md", &["Setup".into()], &original);
        let meddled = text.replace("## 1. ", "## 99. Something Else — ");
        assert_eq!(parse_review(&meddled), original);
    }

    /// Two comments quoting the same words in one section are told apart by
    /// `nth` alone, so it has to survive the file.
    #[test]
    fn two_comments_in_one_section_keep_their_occurrence_indexes() {
        let original = vec![
            Comment::new("1", 2, 0, "retry", "the first one"),
            Comment::new("2", 2, 1, "retry", "the second one"),
        ];
        let text = serialize_review("/tmp/notes.md", &[], &original);
        assert_eq!(parse_review(&text), original);
    }

    #[test]
    fn a_record_with_a_malformed_info_string_is_skipped_not_fatal() {
        for bad in [
            "~~~~ mdview-quote\nq\n~~~~\n",
            "~~~~ mdview-quote a b c\nq\n~~~~\n",
            "~~~~ mdview-quote a 1\nq\n~~~~\n",
            "~~~~ mdview-quote a 1 2 3\nq\n~~~~\n",
        ] {
            assert!(parse_review(bad).is_empty(), "should have skipped {bad:?}");
        }
    }

    /// The discarded record's payload must be consumed with it. If it were
    /// not, a quote reading `~~~~ mdview-quote z 1 0` would be re-read as a
    /// record of its own.
    #[test]
    fn a_skipped_records_payload_is_not_re_read_as_structure() {
        let text = "~~~~~ mdview-quote bad\n~~~~ mdview-quote z 1 0\nsmuggled\n~~~~\n~~~~~\n";
        assert!(parse_review(text).is_empty());
    }

    #[test]
    fn fresh_id_fills_the_first_gap_and_never_collides() {
        assert_eq!(fresh_id(&[]), "1");
        let one = Comment::new("1", 0, 0, "q", "");
        let three = Comment::new("3", 0, 0, "q", "");
        assert_eq!(fresh_id(&[one.clone()]), "2");
        assert_eq!(fresh_id(&[one, three]), "2");
    }

    #[test]
    fn a_comment_normalizes_carriage_returns_and_a_trailing_newline() {
        let comment = Comment::new("1", 0, 0, "a\r\nb\n\n", "note\r\n");
        assert_eq!(comment.quote, "a\nb");
        assert_eq!(comment.note, "note");
    }

    #[test]
    fn the_review_names_the_document_it_is_about() {
        let text = serialize_review("/Users/x/notes/plan.md", &[], &[]);
        assert!(text.starts_with("# Review — plan.md\n/Users/x/notes/plan.md\n"));
    }

    #[test]
    fn a_comment_above_the_first_heading_is_labelled_not_dropped() {
        let comments = vec![Comment::new("1", 0, 0, "preamble", "")];
        let text = serialize_review("/tmp/notes.md", &["Setup".into()], &comments);
        assert!(text.contains("(before the first heading)"));
        assert_eq!(parse_review(&text), comments);
    }
}
