//! Validation of public key installation input.

/// Input that passed validation, as it is sent to the remote side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedInput {
    /// Every input line, in order, as the line reading of [`prepare`] or
    /// [`prepare_verbatim`] gives it, with an LF appended; empty lines at the end
    /// are left out. A CR that ends a line stays in it.
    pub text: Vec<u8>,
    /// Number of key entry lines in `text`.
    pub key_count: usize,
}

/// Reason the input was rejected. `line` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// A line contains `-----BEGIN` and `PRIVATE KEY`.
    PrivateKey { line: usize },
    /// A line contains a NUL byte.
    Nul { line: usize },
    /// A line contains a CR that is not its single trailing CR.
    StandaloneCr { line: usize },
    /// A line is neither blank, a comment, nor a key entry.
    Malformed { line: usize },
    /// All lines are valid but none is a key entry.
    NoKeys,
}

/// Validates `input` as authorized_keys lines and reads them as upstream's
/// `while read -r` does: leading and trailing spaces and tabs are removed, and
/// lines that are then empty at the end are left out. A CR that ends a line is
/// not a space or tab, so it stays, with the spaces and tabs before it.
///
/// A line's content, which the checks classify, excludes one CR at its end. A
/// leading UTF-8 BOM is removed. The private key check runs over all lines
/// before any other check and wins over every other error. Otherwise the first
/// failing line, checked for NUL, then standalone CR, then grammar, is reported.
/// Line numbers count every input line, including the trailing blank lines left out of the text.
pub fn prepare(input: &[u8]) -> Result<PreparedInput, InputError> {
    prepare_lines(input, LineReading::Trimmed)
}

/// Validates `input` as [`prepare`] does but keeps every line as given, as upstream's
/// `$(cat file)` under `-f` does: no spaces, tabs or CRs are removed, and only empty
/// lines at the end are left out. A leading BOM is still removed.
pub fn prepare_verbatim(input: &[u8]) -> Result<PreparedInput, InputError> {
    prepare_lines(input, LineReading::Verbatim)
}

/// The key entry lines of `text`, which passed [`prepare`] or
/// [`prepare_verbatim`], each with its line ending.
pub fn key_lines(text: &[u8]) -> Vec<&[u8]> {
    text.split_inclusive(|&b| b == b'\n')
        .filter(|line| matches!(classify(content_of(line)), Some(LineKind::Key)))
        .collect()
}

/// `line` without its LF and then one CR at its end, which are not part of its content.
fn content_of(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// How the lines that pass validation are written to the prepared text.
#[derive(Clone, Copy)]
enum LineReading {
    /// Leading and trailing spaces and tabs removed, as `read -r` with the default IFS
    /// does; a CR at the end is not one of them.
    Trimmed,
    /// As given.
    Verbatim,
}

fn prepare_lines(input: &[u8], reading: LineReading) -> Result<PreparedInput, InputError> {
    let lines = split_lines(strip_bom(input));
    if let Some(line) = lines.iter().position(|l| is_private_key_marker(l)) {
        return Err(InputError::PrivateKey { line: line + 1 });
    }
    let mut contents = Vec::with_capacity(lines.len());
    let mut key_count = 0;
    for (index, raw) in lines.iter().enumerate() {
        let line = index + 1;
        check_line_bytes(raw, line)?;
        let read = match reading {
            LineReading::Trimmed => trim_separators(raw),
            LineReading::Verbatim => raw,
        };
        match classify(content_of(read)) {
            Some(LineKind::Key) => key_count += 1,
            Some(LineKind::BlankOrComment) => {}
            None => return Err(InputError::Malformed { line }),
        }
        contents.push(read);
    }
    if key_count == 0 {
        return Err(InputError::NoKeys);
    }
    Ok(PreparedInput {
        text: join_without_trailing_blanks(&contents),
        key_count,
    })
}

enum LineKind {
    BlankOrComment,
    Key,
}

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const KEYTYPE_PREFIXES: [&[u8]; 4] = [b"ssh-", b"ecdsa-sha2-", b"sk-ssh-", b"sk-ecdsa-sha2-"];

fn strip_bom(input: &[u8]) -> &[u8] {
    input.strip_prefix(BOM).unwrap_or(input)
}

fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    let body = input.strip_suffix(b"\n").unwrap_or(input);
    if input.is_empty() {
        Vec::new()
    } else {
        body.split(|&b| b == b'\n').collect()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn is_private_key_marker(line: &[u8]) -> bool {
    contains(line, b"-----BEGIN") && contains(line, b"PRIVATE KEY")
}

/// Rejects a line holding a NUL byte or a CR other than one at its end.
fn check_line_bytes(raw: &[u8], line: usize) -> Result<(), InputError> {
    if raw.contains(&0) {
        return Err(InputError::Nul { line });
    }
    if content_of(raw).contains(&b'\r') {
        return Err(InputError::StandaloneCr { line });
    }
    Ok(())
}

fn is_separator(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn skip_separators(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| !is_separator(b)).unwrap_or(s.len());
    &s[start..]
}

/// Removes leading and trailing spaces and tabs, as `read -r` with the default IFS does.
fn trim_separators(s: &[u8]) -> &[u8] {
    let rest = skip_separators(s);
    let end = rest
        .iter()
        .rposition(|&b| !is_separator(b))
        .map_or(0, |i| i + 1);
    &rest[..end]
}

/// Joins the read lines with LF after each, leaving out the empty lines at the end as `$(…)` does.
fn join_without_trailing_blanks(contents: &[&[u8]]) -> Vec<u8> {
    let kept = contents
        .iter()
        .rposition(|c| !c.is_empty())
        .map_or(0, |i| i + 1);
    contents[..kept]
        .iter()
        .flat_map(|c| c.iter().copied().chain(std::iter::once(b'\n')))
        .collect()
}

/// Splits `s` at the first separator into the token and the remainder.
fn next_token(s: &[u8]) -> (&[u8], &[u8]) {
    let end = s.iter().position(|&b| is_separator(b)).unwrap_or(s.len());
    s.split_at(end)
}

/// Splits an options token honoring double quotes and `\"`; `None` when a quote is unterminated.
fn next_options_token(s: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut in_quotes = false;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'\\' if in_quotes && s.get(i + 1) == Some(&b'"') => i += 1,
            b'"' => in_quotes = !in_quotes,
            b if !in_quotes && is_separator(b) => break,
            _ => {}
        }
        i += 1;
    }
    (!in_quotes).then(|| s.split_at(i))
}

fn classify(content: &[u8]) -> Option<LineKind> {
    let rest = skip_separators(content);
    match rest.first() {
        None | Some(b'#') => Some(LineKind::BlankOrComment),
        Some(_) => is_key_entry(rest).then_some(LineKind::Key),
    }
}

fn is_keytype(token: &[u8]) -> bool {
    KEYTYPE_PREFIXES.iter().any(|p| token.starts_with(p))
}

fn is_base64(token: &[u8]) -> bool {
    let data_len = token
        .iter()
        .position(|&b| !(b.is_ascii_alphanumeric() || b == b'+' || b == b'/'))
        .unwrap_or(token.len());
    let padding = &token[data_len..];
    data_len > 0 && padding.len() <= 2 && padding.iter().all(|&b| b == b'=')
}

fn is_key_entry(rest: &[u8]) -> bool {
    let (first, _) = next_token(rest);
    let after_options = if is_keytype(first) {
        rest
    } else {
        match next_options_token(rest) {
            Some((_, remainder)) => skip_separators(remainder),
            None => return false,
        }
    };
    let (keytype, remainder) = next_token(after_options);
    let (base64, _) = next_token(skip_separators(remainder));
    is_keytype(keytype) && is_base64(base64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIA==";

    fn input(template: &str) -> Vec<u8> {
        template.replace("{K}", K).into_bytes()
    }

    fn ok(text: &str, key_count: usize) -> Result<PreparedInput, InputError> {
        Ok(PreparedInput {
            text: input(text),
            key_count,
        })
    }

    fn count(result: Result<PreparedInput, InputError>) -> Result<usize, InputError> {
        result.map(|p| p.key_count)
    }

    #[test]
    fn a01_single_key_with_lf() {
        let given = input("ssh-ed25519 {K} u@h\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} u@h\n", 1));
    }

    #[test]
    fn a02_crlf_is_sent_as_given() {
        let given = input("ssh-ed25519 {K} u@h\r\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} u@h\r\n", 1));
    }

    #[test]
    fn a03_missing_final_lf_is_appended() {
        let given = input("ssh-ed25519 {K} u@h");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} u@h\n", 1));
    }

    #[test]
    fn a04_leading_bom_is_removed() {
        let mut given = vec![0xEF, 0xBB, 0xBF];
        given.extend(input("ssh-ed25519 {K}\n"));
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K}\n", 1));
    }

    #[test]
    fn a05_bom_not_at_start_is_malformed() {
        let mut given = input("ssh-ed25519 {K}\n");
        given.extend([0xEF, 0xBB, 0xBF]);
        given.extend(input("ssh-ed25519 {K}\n"));
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 2 }));
    }

    #[test]
    fn a06_options_with_quoted_space() {
        let given = input("restrict,command=\"echo a b\" ssh-ed25519 {K} c\n");
        assert_eq!(
            prepare(&given),
            ok("restrict,command=\"echo a b\" ssh-ed25519 {K} c\n", 1)
        );
    }

    #[test]
    fn a07_comment_and_blank_lines_are_kept() {
        let given = input("# note\n\nssh-ed25519 {K}\n");
        assert_eq!(prepare(&given), ok("# note\n\nssh-ed25519 {K}\n", 1));
    }

    #[test]
    fn a08_certificate_keytype() {
        let given = input("ssh-ed25519-cert-v01@openssh.com {K} c\n");
        assert_eq!(count(prepare(&given)), Ok(1));
    }

    #[test]
    fn a09_option_without_value() {
        let given = input("cert-authority ssh-rsa {K}\n");
        assert_eq!(count(prepare(&given)), Ok(1));
    }

    #[test]
    fn a10_openssh_private_key() {
        let given =
            input("-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl\n-----END OPENSSH PRIVATE KEY-----\n");
        assert_eq!(prepare(&given), Err(InputError::PrivateKey { line: 1 }));
    }

    #[test]
    fn a11_private_key_wins_over_later_malformed_line() {
        let given = input("ssh-ed25519 {K}\n-----BEGIN RSA PRIVATE KEY-----\nnot a key\n");
        assert_eq!(prepare(&given), Err(InputError::PrivateKey { line: 2 }));
    }

    #[test]
    fn a12_pkcs8_private_key() {
        let given = input("-----BEGIN PRIVATE KEY-----\n");
        assert_eq!(prepare(&given), Err(InputError::PrivateKey { line: 1 }));
    }

    #[test]
    fn a13_encrypted_pkcs8_private_key() {
        let given = input("-----BEGIN ENCRYPTED PRIVATE KEY-----\n");
        assert_eq!(prepare(&given), Err(InputError::PrivateKey { line: 1 }));
    }

    #[test]
    fn a14_nul_byte() {
        let given = input("ssh-ed25519 AA\0AA\n");
        assert_eq!(prepare(&given), Err(InputError::Nul { line: 1 }));
    }

    #[test]
    fn a15_standalone_cr() {
        let given = input("ssh-ed25519 {K} a\rb\n");
        assert_eq!(prepare(&given), Err(InputError::StandaloneCr { line: 1 }));
    }

    #[test]
    fn a16_cr_only_lines_are_blank_and_kept() {
        let given = input("\r\nssh-ed25519 {K}\n\r\n\r\n");
        assert_eq!(prepare(&given), ok("\r\nssh-ed25519 {K}\n\r\n\r\n", 1));
    }

    #[test]
    fn a17_plain_words_are_malformed() {
        let given = input("hello world\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a18_keytype_without_base64() {
        let given = input("ssh-ed25519\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a19_invalid_base64() {
        let given = input("ssh-ed25519 !!!\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a20_unterminated_quote_in_options() {
        let given = input("from=\"a b ssh-ed25519 {K}\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a21_empty_input() {
        assert_eq!(prepare(b""), Err(InputError::NoKeys));
    }

    #[test]
    fn a22_only_comment_and_blank() {
        let given = input("# only\n\n");
        assert_eq!(prepare(&given), Err(InputError::NoKeys));
    }

    #[test]
    fn a23_multiple_keys() {
        let given = input("ssh-ed25519 {K} a\necdsa-sha2-nistp256 {K} b\n");
        assert_eq!(count(prepare(&given)), Ok(2));
    }

    #[test]
    fn a24_leading_spaces_and_tabs_are_removed() {
        let given = input(" \t ssh-ed25519 {K}\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K}\n", 1));
    }

    #[test]
    fn a25_ssh2_public_key_format_is_malformed() {
        let given = input("---- BEGIN SSH2 PUBLIC KEY ----\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a26_excess_padding_is_malformed() {
        let given = input("ssh-ed25519 {K}=== c\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a27_escaped_quote_in_options() {
        let given = input("command=\"say \\\"hi there\\\"\" ssh-ed25519 {K}\n");
        assert_eq!(count(prepare(&given)), Ok(1));
    }

    #[test]
    fn a28_three_padding_characters_are_malformed() {
        let given = input(
            "ssh-ed25519 AAAA=== c
",
        );
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 1 }));
    }

    #[test]
    fn a29_line_is_trimmed_and_trailing_blank_lines_are_dropped() {
        let given = input("  ssh-ed25519 {K} me  \n\n\t\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} me\n", 1));
    }

    #[test]
    fn a30_interior_blank_lines_are_kept_and_emptied() {
        let given = input("ssh-ed25519 {K} a\n \t\n\nssh-ed25519 {K} b\n");
        assert_eq!(
            prepare(&given),
            ok("ssh-ed25519 {K} a\n\n\nssh-ed25519 {K} b\n", 2)
        );
    }

    #[test]
    fn a31_comment_lines_are_trimmed_and_a_trailing_comment_stays() {
        let given = input("\t# c  \nssh-ed25519 {K}\n# d \n\n");
        assert_eq!(prepare(&given), ok("# c\nssh-ed25519 {K}\n# d\n", 1));
    }

    #[test]
    fn a32_whitespace_before_a_cr_is_kept_as_read_keeps_it() {
        let given = input("ssh-ed25519 {K} me \t\r\n \r\n\r\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} me \t\r\n\r\n\r\n", 1));
    }

    #[test]
    fn a33_trailing_blank_lines_without_final_lf_are_dropped() {
        let given = input("ssh-ed25519 {K}\n\n  ");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K}\n", 1));
    }

    #[test]
    fn a34_interior_separators_are_kept() {
        let given = input("ssh-ed25519\t{K}  my  comment\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519\t{K}  my  comment\n", 1));
    }

    #[test]
    fn a35_error_line_numbers_count_leading_blank_and_trimmed_lines() {
        let given = input("\n  \nssh-ed25519 {K}\n  bad line  \n\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 4 }));
    }

    #[test]
    fn v01_verbatim_keeps_leading_and_trailing_separators() {
        let given = input("  ssh-ed25519 {K} me  \n\t# c \n");
        assert_eq!(
            prepare_verbatim(&given),
            ok("  ssh-ed25519 {K} me  \n\t# c \n", 1)
        );
    }

    #[test]
    fn v02_verbatim_drops_only_empty_trailing_lines() {
        let given = input("ssh-ed25519 {K}\n \t\n\n\n");
        assert_eq!(prepare_verbatim(&given), ok("ssh-ed25519 {K}\n \t\n", 1));
    }

    #[test]
    fn v03_verbatim_keeps_the_cr_and_removes_the_bom() {
        let mut given = vec![0xEF, 0xBB, 0xBF];
        given.extend(input(" ssh-ed25519 {K} \r\n"));
        assert_eq!(prepare_verbatim(&given), ok(" ssh-ed25519 {K} \r\n", 1));
    }

    #[test]
    fn v04_verbatim_applies_the_same_checks() {
        let given = input("ssh-ed25519 {K}\n  bad line  \n");
        assert_eq!(
            prepare_verbatim(&given),
            Err(InputError::Malformed { line: 2 })
        );
    }

    #[test]
    fn a36_bom_is_removed_before_trimming() {
        let mut given = vec![0xEF, 0xBB, 0xBF];
        given.extend(input("  ssh-ed25519 {K}\n"));
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K}\n", 1));
    }

    #[test]
    fn c01_crlf_comment_and_blank_lines_are_sent_byte_for_byte() {
        let given = input("# c\r\n\r\nssh-ed25519 {K}\r\n");
        assert_eq!(prepare(&given), ok("# c\r\n\r\nssh-ed25519 {K}\r\n", 1));
    }

    #[test]
    fn c02_cr_at_the_end_of_the_input_is_kept_and_an_lf_added() {
        let given = input("ssh-ed25519 {K}\r");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K}\r\n", 1));
    }

    #[test]
    fn c03_two_crs_before_the_lf_are_a_standalone_cr() {
        let given = input("ssh-ed25519 {K}\r\r\n");
        assert_eq!(prepare(&given), Err(InputError::StandaloneCr { line: 1 }));
    }

    #[test]
    fn c04_trimming_stops_at_a_cr_as_read_does() {
        let given = input("  ssh-ed25519 {K} c \r\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} c \r\n", 1));
    }

    #[test]
    fn c05_verbatim_keeps_a_crlf_line_as_given() {
        let given = input("  ssh-ed25519 {K} c \r\n");
        assert_eq!(prepare_verbatim(&given), ok("  ssh-ed25519 {K} c \r\n", 1));
    }

    #[test]
    fn c06_bom_is_removed_and_the_cr_kept() {
        let mut given = vec![0xEF, 0xBB, 0xBF];
        given.extend(input("ssh-ed25519 {K} c\r\n"));
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} c\r\n", 1));
    }

    #[test]
    fn c07_cr_only_and_crlf_comment_lines_are_not_keys() {
        let given = input("\r\n \t\r\n#x\r\n  # y\r\nssh-ed25519 {K}\r\n");
        assert_eq!(count(prepare(&given)), Ok(1));
        assert_eq!(count(prepare_verbatim(&given)), Ok(1));
    }

    #[test]
    fn c08_cr_ended_line_that_is_not_a_key_is_malformed() {
        let given = input("ssh-ed25519 {K}\r\nhello\r\n");
        assert_eq!(prepare(&given), Err(InputError::Malformed { line: 2 }));
    }

    #[test]
    fn c09_key_lines_are_the_key_entries_with_their_line_endings() {
        let text = input("# c\r\n\r\nssh-ed25519 {K} a\r\n \t\n  ssh-ed25519 {K} b\n");
        assert_eq!(
            key_lines(&text),
            vec![
                input("ssh-ed25519 {K} a\r\n").as_slice(),
                input("  ssh-ed25519 {K} b\n").as_slice()
            ]
        );
    }
}
