//! Validation and normalization of public key installation input.

/// Input that passed validation, normalized for transfer to the remote side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedInput {
    /// Every input line with its trailing CR removed and an LF appended, in order.
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

/// Validates `input` as authorized_keys lines and normalizes it.
///
/// A leading UTF-8 BOM is removed. The private key check runs over all lines
/// before any other check and wins over every other error. Otherwise the first
/// failing line, checked for NUL, then standalone CR, then grammar, is reported.
pub fn prepare(input: &[u8]) -> Result<PreparedInput, InputError> {
    let lines = split_lines(strip_bom(input));
    if let Some(line) = lines.iter().position(|l| is_private_key_marker(l)) {
        return Err(InputError::PrivateKey { line: line + 1 });
    }
    let mut text = Vec::with_capacity(input.len() + 1);
    let mut key_count = 0;
    for (index, raw) in lines.iter().enumerate() {
        let line = index + 1;
        let content = check_line_bytes(raw, line)?;
        match classify(content) {
            Some(LineKind::Key) => key_count += 1,
            Some(LineKind::BlankOrComment) => {}
            None => return Err(InputError::Malformed { line }),
        }
        text.extend_from_slice(content);
        text.push(b'\n');
    }
    if key_count == 0 {
        return Err(InputError::NoKeys);
    }
    Ok(PreparedInput { text, key_count })
}

/// Returns the key entry lines of `prepared.text`, each followed by LF, in order.
///
/// Blank lines and comment lines, indented or not, are dropped. A key line keeps
/// its leading separators.
pub fn key_lines(prepared: &PreparedInput) -> Vec<u8> {
    split_lines(&prepared.text)
        .into_iter()
        .filter(|line| matches!(classify(line), Some(LineKind::Key)))
        .flat_map(|line| [line, b"\n"].concat())
        .collect()
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

/// Returns the line without its trailing CR, or the NUL or standalone CR error.
fn check_line_bytes(raw: &[u8], line: usize) -> Result<&[u8], InputError> {
    if raw.contains(&0) {
        return Err(InputError::Nul { line });
    }
    let content = raw.strip_suffix(b"\r").unwrap_or(raw);
    if content.contains(&b'\r') {
        return Err(InputError::StandaloneCr { line });
    }
    Ok(content)
}

fn is_separator(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn skip_separators(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| !is_separator(b)).unwrap_or(s.len());
    &s[start..]
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
    fn a02_crlf_becomes_lf() {
        let given = input("ssh-ed25519 {K} u@h\r\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K} u@h\n", 1));
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
    fn a16_cr_only_lines_become_blank() {
        let given = input("ssh-ed25519 {K}\n\r\n\r\n");
        assert_eq!(prepare(&given), ok("ssh-ed25519 {K}\n\n\n", 1));
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
    fn a24_leading_spaces_are_kept() {
        let given = input("  ssh-ed25519 {K}\n");
        assert_eq!(prepare(&given), ok("  ssh-ed25519 {K}\n", 1));
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
    fn kl01_key_lines_drop_blank_and_comment_lines() {
        let prepared = prepare(&input("# c\n\n  # indented\n\t\nssh-ed25519 {K}\n")).unwrap();
        assert_eq!(key_lines(&prepared), input("ssh-ed25519 {K}\n"));
    }

    #[test]
    fn kl02_key_lines_keep_leading_spaces() {
        let prepared = prepare(&input("  ssh-ed25519 {K} x\n")).unwrap();
        assert_eq!(key_lines(&prepared), input("  ssh-ed25519 {K} x\n"));
    }
}
