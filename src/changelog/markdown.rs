//! The bits of CommonMark the changelog needs: where fenced code is, and
//! rewriting a changeset body's headings so they sit below the changelog's.
//! actions/release/release-notes.sh follows the same fence rules.

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
    Some((c, run, info.trim().is_empty()))
}

/// One pass over the lines: which are code (fence lines and what's between
/// them), and the index of an opening fence still open at the end, if any.
/// Lines in `skip` are never taken as an opening fence.
fn scan(lines: &[&str], skip: &[bool]) -> (Vec<bool>, Option<usize>) {
    let mut code = vec![false; lines.len()];
    let mut open: Option<(char, usize, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        let marker = fence_marker(line);
        match (open, marker) {
            (Some((c, len, _)), Some((m, run, true))) if m == c && run >= len => {
                open = None;
                code[i] = true;
            }
            (Some(_), _) => code[i] = true,
            (None, Some((c, run, _))) if !skip[i] => {
                open = Some((c, run, i));
                code[i] = true;
            }
            _ => {}
        }
    }
    (code, open.map(|(_, _, i)| i))
}

/// Per line of `text` (split after each '\n'), whether it's fenced code.
/// A fence that's never closed doesn't count: CommonMark would run it to the
/// end of the document, but in a changelog that's a changeset body written
/// before bodies got their fences closed, and it would hide every heading
/// below it. Its opening line is read as plain text instead.
pub(crate) fn code_lines(text: &str) -> Vec<bool> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut skip = vec![false; lines.len()];
    loop {
        match scan(&lines, &skip) {
            (code, None) => return code,
            (_, Some(i)) => skip[i] = true,
        }
    }
}

fn split_eol(line: &str) -> (&str, &str) {
    let text = line.trim_end_matches(['\n', '\r']);
    (text, &line[text.len()..])
}

/// The level of an ATX heading ("## Title", indented at most 3 spaces) and
/// the text after its hashes.
fn atx_heading(text: &str) -> Option<(usize, &str)> {
    let rest = text.trim_start_matches(' ');
    if text.len() - rest.len() > 3 {
        return None;
    }
    let level = rest.chars().take_while(|c| *c == '#').count();
    let after = &rest[level.min(rest.len())..];
    ((1..=6).contains(&level) && (after.is_empty() || after.starts_with([' ', '\t'])))
        .then_some((level, after))
}

/// A Setext underline: "===" or "---" (any length), indented at most 3 spaces.
fn is_setext_underline(text: &str) -> bool {
    let rest = text.trim_start_matches(' ');
    if text.len() - rest.len() > 3 {
        return false;
    }
    let rest = rest.trim_end();
    !rest.is_empty() && (rest.chars().all(|c| c == '=') || rest.chars().all(|c| c == '-'))
}

/// A line that starts a list item or block quote: a paragraph that starts
/// with one can't take a Setext underline, which reads as a thematic break.
fn starts_container(text: &str) -> bool {
    let t = text.trim_start();
    if t.starts_with('>') || t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
}

const ENTRY_LEVEL: usize = 4;

/// Rewrites a changeset body for the changelog so its headings never look
/// like a changelog's own: release-notes.sh and the changelog writer find
/// version, Unreleased and none-bump sections by their "## " and "### "
/// headings, and a body copied in verbatim could carry those. A heading on
/// the body's first line is the entry's title and is written at the entry
/// level, `####`. Any other heading of that level or higher (ATX or Setext)
/// becomes `#####`. Fenced code is left alone, and a fence the body never
/// closes gets closed, so it can't swallow the changelog below it.
pub fn demote_headings(body: &str) -> String {
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let (code, unclosed) = scan(&lines, &vec![false; lines.len()]);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut first = true;
    // The paragraph being read: where it starts in `out` and whether it's the
    // body's first text, or `Some(None)` when it can't be a Setext heading.
    let mut para: Option<Option<(usize, bool)>> = None;
    let heading = |level: usize, is_first: bool| {
        if level > ENTRY_LEVEL {
            level
        } else if is_first {
            ENTRY_LEVEL
        } else {
            ENTRY_LEVEL + 1
        }
    };
    for (i, line) in lines.iter().enumerate() {
        let (text, eol) = split_eol(line);
        if code[i] {
            out.push(line.to_string());
            first = false;
            para = None;
        } else if text.trim().is_empty() {
            out.push(line.to_string());
            para = None;
        } else if let Some((level, after)) = atx_heading(text) {
            out.push(format!("{}{after}{eol}", "#".repeat(heading(level, first))));
            first = false;
            para = None;
        } else if let (true, Some(Some((start, is_first)))) = (is_setext_underline(text), para) {
            let title: Vec<&str> = out[start..].iter().map(|l| l.trim()).collect();
            let title = title.join(" ");
            out.truncate(start);
            out.push(format!("{} {title}{eol}", "#".repeat(heading(1, is_first))));
            para = None;
        } else {
            if para.is_none() {
                para = Some((!starts_container(text)).then_some((out.len(), first)));
            }
            out.push(line.to_string());
            first = false;
        }
    }
    let mut out = out.concat();
    if let Some((c, run, _)) = unclosed.and_then(|i| fence_marker(lines[i])) {
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
    fn indented_atx_headings_are_demoted() {
        assert_eq!(
            demote_headings("#### T\n\n   ## Three spaces\n\n    ## Four is code\n"),
            "#### T\n\n##### Three spaces\n\n    ## Four is code\n"
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
    fn thematic_breaks_and_lists_are_not_setext() {
        let body = "#### T\n\nText\n\n---\n\n- item\n---\n\n> quote\n===\n";
        assert_eq!(demote_headings(body), body);
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
        assert_eq!(code_lines("    ```\n## a\n"), vec![false, false]);
        assert_eq!(code_lines("``` a`b\n## a\n"), vec![false, false]);
        assert_eq!(code_lines("~~~ a`b\n## a\n~~~\n"), vec![true, true, true]);
        assert_eq!(
            code_lines("```\n## a\n``` x\n```\n"),
            vec![true, true, true, true]
        );
    }

    #[test]
    fn an_unclosed_fence_is_read_as_text() {
        assert_eq!(
            code_lines("```sh\n## 1.0.0\n~~~\ncode\n~~~\n"),
            vec![false, false, true, true, true]
        );
    }
}
