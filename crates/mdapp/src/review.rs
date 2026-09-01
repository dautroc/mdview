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

/// Why one record could not be read. Short enough for a banner, specific
/// enough to find the record it is about.
pub const BAD_INFO: &str =
    "its `~~~~ mdview-quote` line is missing the id, heading and occurrence numbers";
pub const ORPHAN_NOTE: &str = "a `~~~~ mdview-note` block with no comment above it to attach to";
pub const MERGED: &str = "its closing fence is missing, so it has swallowed the record below it";

/// A record `parse_review` had to skip, and where to find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damage {
    /// 1-based line of the fence that opens the unreadable record.
    pub line: usize,
    pub reason: &'static str,
}

/// One review file: the comments, and whatever could not be read.
///
/// The second half exists because writing is destructive. `serialize_review`
/// renders the comment list and nothing else, so writing a file we only partly
/// understood erases every record we skipped — and this file is shared with
/// Claude, which `C` now asks to edit it on every pass. Silence there costs
/// someone a comment with nothing to show that it ever existed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Review {
    pub comments: Vec<Comment>,
    pub damage: Vec<Damage>,
}

/// Has this payload swallowed a record whose opening fence is still in it?
///
/// The test is `width`, and it is exact rather than a guess. `fence_for` makes
/// every fence strictly longer than the longest run of leading tildes in the
/// payload it wraps, so in a file MDView wrote, no payload line can open a
/// fence as wide as the one enclosing it. A line that does means two records
/// were merged — which is what deleting a closing fence does, since the NEXT
/// record's closing fence then closes this one and nothing looks unterminated.
///
/// The strictness matters in both directions: a quote whose text happens to
/// contain `~~~~ mdview-quote` is legitimate, and is wrapped in a longer fence
/// precisely so it can be told apart from this.
fn swallows_a_record(payload: &[&str], width: usize) -> bool {
    payload.iter().any(|line| match opener(line) {
        Some(open) => {
            open.width >= width && (open.kind == "mdview-quote" || open.kind == "mdview-note")
        }
        None => false,
    })
}

/// Parse a review file. Never fails: a malformed record is skipped and a
/// truncated file yields the records it could read, matching the contract
/// `parse_message` states in `state.rs`. Dropping the whole file because one
/// record is bad would lose comments a human still has on screen.
///
/// What it will not do is skip a record *quietly*. Every skip is recorded in
/// `Review::damage`, and the caller refuses to write the file while any of it
/// stands.
pub fn parse_review(text: &str) -> Review {
    let lines: Vec<&str> = text.lines().collect();
    let mut review = Review::default();
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
        let at = index + 1;
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

        // A fence that simply runs off the end of the file is a write that was
        // cut short, and the payload is what survived it — kept, as it always
        // was. A fence that has eaten the record below it is a different thing
        // entirely, and that record has stopped existing.
        if swallows_a_record(&payload, open.width) {
            review.damage.push(Damage { line: at, reason: MERGED });
        }

        if open.kind == "mdview-note" {
            // Attaches to the most recent quote that has not been given one,
            // so the decorative headings between them do not break the pair.
            let attached = match review.comments.last_mut() {
                Some(last) if last.note.is_empty() => {
                    last.note = body;
                    true
                }
                _ => false,
            };
            // Its quote block is gone, or the comment above it already has a
            // note. Either way this text has nowhere to go.
            if !attached {
                review.damage.push(Damage { line: at, reason: ORPHAN_NOTE });
            }
            continue;
        }
        let [id, heading, nth] = open.info.as_slice() else {
            review.damage.push(Damage { line: at, reason: BAD_INFO });
            continue;
        };
        let (Ok(heading), Ok(nth)) = (heading.parse::<usize>(), nth.parse::<usize>()) else {
            review.damage.push(Damage { line: at, reason: BAD_INFO });
            continue;
        };
        review.comments.push(Comment::new(id, heading, nth, &body, ""));
    }
    review
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse, insisting the file was wholly readable. Every test that builds
    /// a well-formed file goes through this, so each of them also pins that
    /// MDView would be willing to write the file back.
    fn read(text: &str) -> Vec<Comment> {
        let review = parse_review(text);
        assert_eq!(review.damage, vec![], "unexpected damage in a well-formed file");
        review.comments
    }

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
                    read(&text),
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
        assert_eq!(read(&annotated), original);
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
        let review = parse_review(cut);
        assert_eq!(review.comments, original);
        // And a cut-short write is not damage: the payload is what survived,
        // nothing was swallowed, and refusing to write from here on would
        // strand the file for good.
        assert_eq!(review.damage, vec![]);
    }

    #[test]
    fn a_record_with_no_note_block_parses_as_an_empty_note() {
        let text = "~~~~ mdview-quote a 4 2\njust the quote\n~~~~\n";
        assert_eq!(
            read(text),
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
        assert_eq!(read(&meddled), original);
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
        assert_eq!(read(&text), original);
    }

    #[test]
    fn a_record_with_a_malformed_info_string_is_skipped_not_fatal() {
        for bad in [
            "~~~~ mdview-quote\nq\n~~~~\n",
            "~~~~ mdview-quote a b c\nq\n~~~~\n",
            "~~~~ mdview-quote a 1\nq\n~~~~\n",
            "~~~~ mdview-quote a 1 2 3\nq\n~~~~\n",
        ] {
            let review = parse_review(bad);
            assert!(review.comments.is_empty(), "should have skipped {bad:?}");
            // Skipped, but never quietly: the payload is someone's comment and
            // writing the file would erase it.
            assert_eq!(
                review.damage,
                vec![Damage { line: 1, reason: BAD_INFO }],
                "silently dropped {bad:?}"
            );
        }
    }

    /// The discarded record's payload must be consumed with it. If it were
    /// not, a quote reading `~~~~ mdview-quote z 1 0` would be re-read as a
    /// record of its own.
    #[test]
    fn a_skipped_records_payload_is_not_re_read_as_structure() {
        let text = "~~~~~ mdview-quote bad\n~~~~ mdview-quote z 1 0\nsmuggled\n~~~~\n~~~~~\n";
        let review = parse_review(text);
        assert!(review.comments.is_empty());
        assert_eq!(review.damage, vec![Damage { line: 1, reason: BAD_INFO }]);
    }

    /// The whole point of the exercise, done right: `C` tells Claude to delete
    /// both fenced blocks of each comment it addressed, and doing exactly that
    /// has to leave a file MDView will still write to. This is the case that
    /// happens on every successful pass, so a false positive here would stop
    /// comments being saved for the entire feature.
    #[test]
    fn deleting_a_record_the_way_the_prompt_asks_leaves_a_readable_file() {
        let whole = vec![
            Comment::new("1", 1, 0, "first quote", "first note"),
            Comment::new("2", 2, 0, "second quote", "second note"),
            Comment::new("3", 3, 0, "third quote", "third note"),
        ];
        let text = serialize_review("/tmp/notes.md", &[], &whole);
        // Both blocks of record 2, opening and closing fences included, which
        // is what the prompt spells out.
        let quote = "~~~~ mdview-quote 2 2 0\nsecond quote\n~~~~\n";
        let note = "~~~~ mdview-note\nsecond note\n~~~~\n";
        assert!(text.contains(quote) && text.contains(note), "the file is not shaped as assumed");
        let pruned = text.replace(quote, "").replace(note, "");

        let review = parse_review(&pruned);
        assert_eq!(review.comments, vec![whole[0].clone(), whole[2].clone()]);
        assert_eq!(review.damage, vec![], "a correct deletion must not read as damage");
    }

    /// The half-deletion `C` invites: Claude removes the quote block of the
    /// comment it addressed and leaves the note behind. The note has nothing
    /// to attach to, so its text would be dropped on read and gone from the
    /// file on the next write.
    #[test]
    fn a_note_block_whose_quote_was_deleted_is_reported_not_dropped() {
        let text = "~~~~ mdview-note\nthe note nobody deleted\n~~~~\n";
        let review = parse_review(text);
        assert!(review.comments.is_empty());
        assert_eq!(review.damage, vec![Damage { line: 1, reason: ORPHAN_NOTE }]);
    }

    /// The same edit made one record too far down: the note now lands on the
    /// comment ABOVE it, which already has one of its own. Silently keeping
    /// the first note and discarding the second is the worst of both.
    #[test]
    fn a_second_note_on_one_comment_is_reported_rather_than_discarded() {
        let text = concat!(
            "~~~~ mdview-quote a 1 0\nquote\n~~~~\n",
            "~~~~ mdview-note\nthe first note\n~~~~\n",
            "~~~~ mdview-note\nthe orphaned one\n~~~~\n",
        );
        let review = parse_review(text);
        assert_eq!(review.comments, vec![Comment::new("a", 1, 0, "quote", "the first note")]);
        assert_eq!(review.damage, vec![Damage { line: 7, reason: ORPHAN_NOTE }]);
    }

    /// The other half-deletion: the closing fence goes. Nothing looks
    /// unterminated afterwards — the NEXT record's closing fence quietly
    /// closes this one — so the two records become one holding both payloads,
    /// and the second has stopped existing. A write would make that permanent.
    #[test]
    fn a_deleted_closing_fence_that_swallows_the_record_below_it_is_reported() {
        let whole = vec![
            Comment::new("a", 1, 0, "quote", ""),
            Comment::new("b", 2, 0, "second quote", ""),
        ];
        let text = serialize_review("/tmp/notes.md", &[], &whole);
        // Exactly one closing fence removed, the way a careless edit would.
        let broken = text.replacen("quote\n~~~~\n", "quote\n", 1);
        let review = parse_review(&broken);
        assert_eq!(review.comments.len(), 1, "the two records merged into one");
        assert_eq!(review.damage.len(), 1);
        assert_eq!(review.damage[0].reason, MERGED);
    }

    /// The same edit at the end of the file, where there is no following fence
    /// to close it: still one record swallowing another.
    #[test]
    fn a_missing_final_fence_that_swallows_a_record_is_reported() {
        let text = concat!(
            "~~~~ mdview-quote a 1 0\nquote\n",
            "~~~~ mdview-note\nthe note\n",
        );
        let review = parse_review(text);
        assert_eq!(review.damage, vec![Damage { line: 1, reason: MERGED }]);
    }

    /// The distinction the rule turns on, from the other side: a fence that
    /// runs to the end of a cut-short file swallowed nothing, and is the
    /// tolerated case. Getting this wrong would make every truncated file
    /// unwritable rather than merely short.
    #[test]
    fn a_fence_that_runs_off_the_end_of_a_cut_file_swallows_nothing() {
        let review = parse_review("~~~~ mdview-quote a 1 0\nhalf a quo");
        assert_eq!(review.comments, vec![Comment::new("a", 1, 0, "half a quo", "")]);
        assert_eq!(review.damage, vec![]);
    }

    /// The false positive the width test exists to avoid: a quote whose own
    /// text is a record opener. `fence_for` wrapped it in a longer fence, which
    /// is exactly what tells it apart from a record that swallowed another.
    #[test]
    fn a_quote_whose_text_is_itself_a_record_opener_is_not_damage() {
        for payload in [
            "~~~~ mdview-quote z 9 9\nnot really a record",
            "~~~~ mdview-note\nnor is this",
            "~~~~~~ mdview-quote z 9 9",
        ] {
            let original = vec![c(payload, "a note about it")];
            let text = serialize_review("/tmp/notes.md", &[], &original);
            assert_eq!(read(&text), original, "false positive on {payload:?}");
        }
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
        assert_eq!(read(&text), comments);
    }
}
