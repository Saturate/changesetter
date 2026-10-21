//! The bits of CommonMark a changeset body needs when it's written to the
//! changelog: where its fenced code is, and rewriting its headings so they
//! sit below the changelog's own.
//!
//! Only the writer looks at fences. The changelog parsers (the Rust ones and
//! actions/release/release-notes.sh) read plain lines, so they stay linear and
//! read old changelogs the same way they always have.

/// A fence line: up to 3 spaces of indent, then 3 or more backticks or
/// tildes. A backtick fence's info string can't contain a backtick. Returns
/// the character, the run length, and whether nothing follows the run (only
/// such a line can close a fence).
fn fence_marker(line: &str) -> Option<(char, usize, bool)> {
    let line = line.trim_end_matches(['\n', '\r']);
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = rest.chars().take_while(|x| *x == c).count();
    if run < 3 {
        return None;
    }
    let info = &rest[run..];
    if c == '`' && info.contains('`') {
        return None;
    }
    Some((c, run, info.trim_matches([' ', '\t']).is_empty()))
}

/// One pass over the lines: which are fenced code (fence lines and what's
/// between them), and the opening fence still open at the end, if any. An
/// unclosed fence runs to the end, as CommonMark reads it.
fn scan(lines: &[&str]) -> (Vec<bool>, Option<(char, usize)>) {
    let mut code = vec![false; lines.len()];
    let mut open: Option<(char, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        match (open, fence_marker(line)) {
            (Some((c, len)), Some((m, run, true))) if m == c && run >= len => {
                open = None;
                code[i] = true;
            }
            (Some(_), _) => code[i] = true,
            (None, Some((c, run, _))) => {
                open = Some((c, run));
                code[i] = true;
            }
            _ => {}
        }
    }
    (code, open)
}

/// Per line of `text` (split after each '\n'), whether it's fenced code.
pub(crate) fn fenced_lines(text: &str) -> Vec<bool> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    scan(&lines).0
}

fn split_eol(line: &str) -> (&str, &str) {
    let text = line.trim_end_matches(['\n', '\r']);
    (text, &line[text.len()..])
}

/// An ATX heading ("## Title", indented at most 3 spaces): its indent, level
/// and the text after its hashes.
fn atx_heading(text: &str) -> Option<(&str, usize, &str)> {
    let rest = text.trim_start_matches(' ');
    let indent = &text[..text.len() - rest.len()];
    if indent.len() > 3 {
        return None;
    }
    let level = rest.chars().take_while(|c| *c == '#').count();
    let after = &rest[level.min(rest.len())..];
    ((1..=6).contains(&level) && (after.is_empty() || after.starts_with([' ', '\t'])))
        .then_some((indent, level, after))
}

/// A Setext underline: "===" or "---" (any length), indented at most 3 spaces.
fn is_setext_underline(text: &str) -> bool {
    let rest = text.trim_start_matches(' ');
    if text.len() - rest.len() > 3 {
        return false;
    }
    let rest = rest.trim_end_matches([' ', '\t']);
    !rest.is_empty() && (rest.chars().all(|c| c == '=') || rest.chars().all(|c| c == '-'))
}

/// A line that starts a block other than a paragraph: a list item, block
/// quote or table row. It can't be part of a Setext heading's text, and in
/// CommonMark a list item or quote interrupts a paragraph, so a "---" or "==="
/// below one isn't a Setext underline.
fn starts_other_block(text: &str) -> bool {
    let t = text.trim_start_matches(' ');
    if t.starts_with('>') || t.starts_with('|') {
        return true;
    }
    if ["- ", "* ", "+ "].iter().any(|m| t.starts_with(m)) || ["-", "*", "+"].contains(&t) {
        return true;
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    (1..=9).contains(&digits) && [". ", ") "].iter().any(|m| t[digits..].starts_with(m))
}

const ENTRY_LEVEL: usize = 4;

fn demoted_level(level: usize, is_first: bool) -> usize {
    if level > ENTRY_LEVEL {
        level
    } else if is_first {
        ENTRY_LEVEL
    } else {
        ENTRY_LEVEL + 1
    }
}

/// Rewrites a changeset body for the changelog so its headings never look
/// like a changelog's own: release-notes.sh and the changelog writer find
/// version, Unreleased and none-bump sections by their "## " and "### "
/// headings, and a body copied in verbatim could carry those. A heading on
/// the body's first line is the entry's title and is written at the entry
/// level, `####`. Any other heading of that level or higher (ATX or Setext)
/// becomes `#####`, keeping its indent. Fenced code is left alone, and a fence
/// the body never closes gets closed, so the rest of the changelog doesn't
/// render as code.
pub fn demote_headings(body: &str) -> String {
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let (code, unclosed) = scan(&lines);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut first = true;
    // The paragraph being read: where it starts in `out` and whether it's the
    // body's first text. `Some(None)` is a run of lines that can't take a
    // Setext underline (a list, quote or table).
    let mut para: Option<Option<(usize, bool)>> = None;
    for (i, line) in lines.iter().enumerate() {
        let (text, eol) = split_eol(line);
        if code[i] {
            out.push(line.to_string());
            first = false;
            para = None;
        } else if text.trim_matches([' ', '\t']).is_empty() {
            out.push(line.to_string());
            para = None;
        } else if para.is_none() && text.len() - text.trim_start_matches(' ').len() >= 4 {
            // Indented code: not a paragraph, so no Setext heading over it.
            out.push(line.to_string());
            first = false;
        } else if crate::changelog::generator::is_link_reference(text) {
            // A version link reference would end the entry's section for the
            // parsers. One space of indent keeps it a link reference definition
            // (CommonMark allows up to three) and no parser matches it.
            out.push(format!(" {line}"));
            first = false;
            para = None;
        } else if let Some((indent, level, after)) = atx_heading(text) {
            let hashes = "#".repeat(demoted_level(level, first));
            out.push(format!("{indent}{hashes}{after}{eol}"));
            first = false;
            para = None;
        } else if let (true, Some(Some((start, is_first)))) = (is_setext_underline(text), para) {
            let indent = &out[start][..out[start].len() - out[start].trim_start_matches(' ').len()];
            let indent = indent.to_string();
            let title: Vec<&str> = out[start..]
                .iter()
                .map(|l| l.trim_matches([' ', '\t', '\r', '\n']))
                .collect();
            let title = title.join(" ");
            out.truncate(start);
            let hashes = "#".repeat(demoted_level(1, is_first));
            out.push(format!("{indent}{hashes} {title}{eol}"));
            para = None;
        } else {
            if starts_other_block(text) {
                para = Some(None);
            } else if para.is_none() {
                para = Some(Some((out.len(), first)));
            }
            out.push(line.to_string());
            first = false;
        }
    }
    let mut out = out.concat();
    if let Some((c, run)) = unclosed {
        let ends_with_newline = out.ends_with('\n');
        if !ends_with_newline {
            out.push('\n');
        }
        out.push_str(&c.to_string().repeat(run));
        if ends_with_newline {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_headings_go_below_the_entry_heading() {
        let body = "#### CI one\n\nDetails:\n\n### Sub heading\n\nmore\n\n## Big\n\n# Top\n\n#### Same\n\n##### Deep\n\n###### Deeper\n";
        assert_eq!(
            demote_headings(body),
            "#### CI one\n\nDetails:\n\n##### Sub heading\n\nmore\n\n##### Big\n\n##### Top\n\n##### Same\n\n##### Deep\n\n###### Deeper\n"
        );
    }

    #[test]
    fn a_title_written_higher_goes_to_the_entry_level() {
        assert_eq!(
            demote_headings("\n## Title\n\nText\n"),
            "\n#### Title\n\nText\n"
        );
        assert_eq!(
            demote_headings("Text\n\n## Later\n"),
            "Text\n\n##### Later\n"
        );
    }

    #[test]
    fn fenced_code_is_left_alone() {
        let body = "#### T\n\n```md\n## not a heading\n~~~\n# still code\n```\n\n~~~~\n### code\n~~~~\n\n## after\n";
        assert_eq!(
            demote_headings(body),
            "#### T\n\n```md\n## not a heading\n~~~\n# still code\n```\n\n~~~~\n### code\n~~~~\n\n##### after\n"
        );
    }

    #[test]
    fn hashes_without_a_space_are_text() {
        assert_eq!(demote_headings("#hashtag\n##\n"), "#hashtag\n#####\n");
    }

    #[test]
    fn indented_headings_keep_their_indent() {
        assert_eq!(
            demote_headings("#### T\n\n- item\n\n   ## In the item\n\n    ## Four is code\n"),
            "#### T\n\n- item\n\n   ##### In the item\n\n    ## Four is code\n"
        );
    }

    #[test]
    fn setext_headings_are_demoted() {
        assert_eq!(
            demote_headings("Title\n=====\n\nText\n\nSub part\none\n---\n\nEnd\n"),
            "#### Title\n\nText\n\n##### Sub part one\n\nEnd\n"
        );
    }

    #[test]
    fn thematic_breaks_lists_quotes_and_tables_are_not_setext() {
        for body in [
            "#### T\n\nText\n\n---\n",
            "#### T\n\n- item\n---\n",
            "#### T\n\nSteps:\n- one\n- two\n---\n",
            "#### T\n\nSee\n> quoted\n===\n",
            "#### T\n\n1. one\n---\n",
            "#### T\n\n| a | b |\n---\n",
        ] {
            assert_eq!(demote_headings(body), body, "{body:?}");
        }
    }

    #[test]
    fn version_link_references_get_one_space_of_indent() {
        assert_eq!(
            demote_headings(
                "Fix, see [1.0.0].\n\n[1.0.0]: https://x/1.0.0\n[Unreleased]: https://x\n[docs]: https://x/docs\n\nTail\n"
            ),
            "Fix, see [1.0.0].\n\n [1.0.0]: https://x/1.0.0\n [Unreleased]: https://x\n[docs]: https://x/docs\n\nTail\n"
        );
        // In code it's left alone (and warned about instead).
        assert_eq!(
            demote_headings("```\n[1.0.0]: x\n```\n"),
            "```\n[1.0.0]: x\n```\n"
        );
    }

    #[test]
    fn indented_code_is_not_a_setext_heading() {
        for body in ["Example:\n\n    let x = 1;\n---\n", "    let x = 1;\n===\n"] {
            assert_eq!(demote_headings(body), body, "{body:?}");
        }
        // A 4-space line inside a paragraph is still paragraph text.
        assert_eq!(
            demote_headings("Title\n    more\n===\n"),
            "#### Title more\n"
        );
    }

    #[test]
    fn an_unclosed_fence_is_closed() {
        assert_eq!(
            demote_headings("Fix\n\n```sh\necho unclosed\n"),
            "Fix\n\n```sh\necho unclosed\n```\n"
        );
        assert_eq!(demote_headings("Fix\n\n~~~~\nx"), "Fix\n\n~~~~\nx\n~~~~");
    }

    #[test]
    fn fence_rules_follow_commonmark() {
        // Indented 4 spaces is an indented code block, and a backtick fence's
        // info string can't hold a backtick.
        assert_eq!(fenced_lines("    ```\n## a\n"), vec![false, false]);
        assert_eq!(fenced_lines("``` a`b\n## a\n"), vec![false, false]);
        assert_eq!(fenced_lines("~~~ a`b\n## a\n~~~\n"), vec![true, true, true]);
        assert_eq!(
            fenced_lines("```\n## a\n``` x\n```\n"),
            vec![true, true, true, true]
        );
        assert_eq!(fenced_lines("```\n## a\n"), vec![true, true]);
    }
}
