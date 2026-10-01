//! Encoding of result lines printed by the remote installation script, and derivation of the outcome from them.

/// Overall outcome derived from the remote script's result lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    Installed,
    Partial,
    /// A failed write could not be confirmed as rolled back; the target may hold a partial line.
    Uncertain,
    Unknown,
}

/// Per-key result reported by the remote script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    Added,
    Skipped,
    Failed,
    /// The key's write failed and its rollback could not be confirmed.
    Uncertain,
}

/// One well-formed key line: 1-based key index, its status, and the decoded target path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyResult {
    pub index: usize,
    pub status: KeyStatus,
    pub path: Vec<u8>,
}

/// Outcome of a run and every well-formed key line in output order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub outcome: Outcome,
    pub keys: Vec<KeyResult>,
}

/// Encodes a path for a result line.
///
/// Bytes in `0x21..=0x7E` other than `%` and `=` are written as themselves;
/// every other byte is written as `%` followed by two uppercase hex digits.
pub fn encode_path(path: &[u8]) -> String {
    path.iter()
        .map(|&b| {
            if is_literal(b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn is_literal(b: u8) -> bool {
    (0x21..=0x7E).contains(&b) && b != b'%' && b != b'='
}

fn upper_hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decodes a path written by [`encode_path`].
///
/// Returns `None` when `%` is not followed by exactly two uppercase hex digits,
/// or when a raw character appears that [`encode_path`] would have escaped.
pub fn decode_path(encoded: &str) -> Option<Vec<u8>> {
    let mut bytes = encoded.bytes();
    let mut decoded = Vec::with_capacity(encoded.len());
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let high = upper_hex_value(bytes.next()?)?;
            let low = upper_hex_value(bytes.next()?)?;
            decoded.push(high << 4 | low);
        } else if is_literal(b) {
            decoded.push(b);
        } else {
            return None;
        }
    }
    Some(decoded)
}

const PREFIX: &[u8] = b"ssh-copy-id: ";

enum Line {
    Key(KeyResult),
    Summary(Outcome, usize),
}

fn parse_decimal(text: &str) -> Option<usize> {
    let well_formed = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if well_formed { text.parse().ok() } else { None }
}

fn parse_key_line(body: &str) -> Option<KeyResult> {
    let [index, status, path] = fields(body)?;
    let index = parse_decimal(index.strip_prefix("key=")?).filter(|&n| n >= 1)?;
    let status = match status.strip_prefix("result=")? {
        "added" => KeyStatus::Added,
        "skipped" => KeyStatus::Skipped,
        "failed" => KeyStatus::Failed,
        "uncertain" => KeyStatus::Uncertain,
        _ => return None,
    };
    let path = decode_path(path.strip_prefix("path=")?)?;
    Some(KeyResult {
        index,
        status,
        path,
    })
}

fn parse_summary_line(body: &str) -> Option<(Outcome, usize)> {
    let [result, added] = fields(body)?;
    let outcome = match result.strip_prefix("result=")? {
        "unchanged" => Outcome::Unchanged,
        "installed" => Outcome::Installed,
        "partial" => Outcome::Partial,
        "uncertain" => Outcome::Uncertain,
        _ => return None,
    };
    let added = parse_decimal(added.strip_prefix("added=")?)?;
    Some((outcome, added))
}

fn fields<const N: usize>(body: &str) -> Option<[&str; N]> {
    body.split(' ').collect::<Vec<_>>().try_into().ok()
}

fn parse_line(body: &[u8]) -> Option<Line> {
    let body = std::str::from_utf8(body).ok()?;
    parse_key_line(body)
        .map(Line::Key)
        .or_else(|| parse_summary_line(body).map(|(o, n)| Line::Summary(o, n)))
}

fn prefixed_bodies(stdout: &[u8]) -> impl Iterator<Item = &[u8]> {
    stdout.split(|&b| b == b'\n').filter_map(|line| {
        line.strip_suffix(b"\r")
            .unwrap_or(line)
            .strip_prefix(PREFIX)
    })
}

fn derive_outcome(lines: &[Option<Line>], keys: &[KeyResult]) -> Outcome {
    let Some(Some(Line::Summary(outcome, added))) = lines.last() else {
        return Outcome::Unknown;
    };
    let all_but_last_are_keys = lines[..lines.len() - 1]
        .iter()
        .all(|line| matches!(line, Some(Line::Key(_))));
    let count = |status| keys.iter().filter(|k| k.status == status).count();
    let consistent = *added == count(KeyStatus::Added)
        && !(*outcome == Outcome::Installed && count(KeyStatus::Failed) > 0)
        && !(*outcome == Outcome::Unchanged && *added != 0)
        && !(*outcome != Outcome::Uncertain && count(KeyStatus::Uncertain) > 0);
    if all_but_last_are_keys && consistent {
        *outcome
    } else {
        Outcome::Unknown
    }
}

/// Parses the remote script's stdout into a [`Report`].
///
/// Lines are split on LF with one trailing CR removed; lines without the
/// `ssh-copy-id: ` prefix are ignored. `keys` holds every well-formed key line
/// in order. The outcome is the summary's value only when exactly one
/// well-formed summary line is the last prefixed line, every prefixed line is
/// well-formed, `added` equals the number of added key lines, `installed` has
/// no failed key, `unchanged` has `added=0`, and only `uncertain` has an
/// uncertain key; otherwise it is
/// [`Outcome::Unknown`].
pub fn parse_report(stdout: &[u8]) -> Report {
    let lines: Vec<Option<Line>> = prefixed_bodies(stdout).map(parse_line).collect();
    let keys: Vec<KeyResult> = lines
        .iter()
        .filter_map(|line| match line {
            Some(Line::Key(key)) => Some(key.clone()),
            _ => None,
        })
        .collect();
    Report {
        outcome: derive_outcome(&lines, &keys),
        keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(index: usize, status: KeyStatus, path: &[u8]) -> KeyResult {
        KeyResult {
            index,
            status,
            path: path.to_vec(),
        }
    }

    #[test]
    fn b01_encode_plain_path_is_unchanged() {
        assert_eq!(
            encode_path(b"/home/u/.ssh/authorized_keys"),
            "/home/u/.ssh/authorized_keys"
        );
    }

    #[test]
    fn b02_encode_escapes_space_equals_and_percent() {
        assert_eq!(encode_path(b"a b=c%d"), "a%20b%3Dc%25d");
    }

    #[test]
    fn b03_encode_escapes_control_bytes() {
        assert_eq!(encode_path(b"x\r\ny\t"), "x%0D%0Ay%09");
    }

    #[test]
    fn b04_encode_escapes_non_ascii_and_del() {
        assert_eq!(encode_path(&[0xE6, 0x97, 0xA5, 0x7F]), "%E6%97%A5%7F");
    }

    #[test]
    fn b05_decode_escapes() {
        assert_eq!(decode_path("a%20b%3Dc%25d"), Some(b"a b=c%d".to_vec()));
    }

    #[test]
    fn b06_decode_rejects_lowercase_hex() {
        assert_eq!(decode_path("%e6"), None);
    }

    #[test]
    fn b07_decode_rejects_short_escape() {
        assert_eq!(decode_path("%4"), None);
    }

    #[test]
    fn b08_decode_rejects_non_hex_escape() {
        assert_eq!(decode_path("%ZZ"), None);
    }

    #[test]
    fn b09_decode_rejects_raw_space() {
        assert_eq!(decode_path("a b"), None);
    }

    #[test]
    fn b10_round_trip_every_single_byte() {
        for b in 0..=255u8 {
            let p = [b];
            assert_eq!(
                decode_path(&encode_path(&p)),
                Some(p.to_vec()),
                "byte {b:#04x}"
            );
        }
    }

    #[test]
    fn b11_single_added_key_installed() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=added path=/x\nssh-copy-id: result=installed added=1\n",
        );
        assert_eq!(
            r,
            Report {
                outcome: Outcome::Installed,
                keys: vec![key(1, KeyStatus::Added, b"/x")],
            }
        );
    }

    #[test]
    fn b12_skipped_and_added_installed() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=skipped path=/x\n\
              ssh-copy-id: key=2 result=added path=/x\n\
              ssh-copy-id: result=installed added=1\n",
        );
        assert_eq!(r.outcome, Outcome::Installed);
        assert_eq!(
            r.keys,
            vec![
                key(1, KeyStatus::Skipped, b"/x"),
                key(2, KeyStatus::Added, b"/x"),
            ]
        );
    }

    #[test]
    fn b13_no_summary_is_unknown_but_keeps_keys() {
        let r = parse_report(b"ssh-copy-id: key=1 result=added path=/x\n");
        assert_eq!(
            r,
            Report {
                outcome: Outcome::Unknown,
                keys: vec![key(1, KeyStatus::Added, b"/x")],
            }
        );
    }

    #[test]
    fn b14_prefixed_line_after_summary_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: result=installed added=1\nssh-copy-id: key=1 result=added path=/x\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b15_noise_and_crlf_are_ignored() {
        let r = parse_report(
            b"Windows PowerShell\r\n\
              ssh-copy-id: key=1 result=added path=/x\r\n\
              ssh-copy-id: result=installed added=1\r\n\
              bye\n",
        );
        assert_eq!(
            r,
            Report {
                outcome: Outcome::Installed,
                keys: vec![key(1, KeyStatus::Added, b"/x")],
            }
        );
    }

    #[test]
    fn b16_added_count_mismatch_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=added path=/x\nssh-copy-id: result=installed added=2\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b17_malformed_key_index_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=x result=added path=/x\nssh-copy-id: result=installed added=0\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b18_added_and_failed_partial() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=added path=/x\n\
              ssh-copy-id: key=2 result=failed path=/x\n\
              ssh-copy-id: result=partial added=1\n",
        );
        assert_eq!(r.outcome, Outcome::Partial);
    }

    #[test]
    fn b19_skipped_only_unchanged() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=skipped path=/x\nssh-copy-id: result=unchanged added=0\n",
        );
        assert_eq!(r.outcome, Outcome::Unchanged);
    }

    #[test]
    fn b20_invalid_path_encoding_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=added path=%zz\nssh-copy-id: result=installed added=1\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b21_empty_stdout_is_unknown() {
        assert_eq!(
            parse_report(b""),
            Report {
                outcome: Outcome::Unknown,
                keys: vec![],
            }
        );
    }

    #[test]
    fn b22_installed_with_failed_key_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=failed path=/x\nssh-copy-id: result=installed added=0\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b23_unchanged_with_nonzero_added_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=added path=/x\nssh-copy-id: result=unchanged added=1\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b24_two_summary_lines_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: result=installed added=0\nssh-copy-id: result=installed added=0\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b25_leading_zero_key_index_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=01 result=added path=/x\nssh-copy-id: result=installed added=1\n",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b26_zero_key_index_is_unknown() {
        let r = parse_report(
            b"ssh-copy-id: key=0 result=added path=/x
ssh-copy-id: result=installed added=1
",
        );
        assert_eq!(r.outcome, Outcome::Unknown);
    }

    #[test]
    fn b27_uncertain_key_line_is_parsed() {
        let r = parse_report(
            b"ssh-copy-id: key=1 result=uncertain path=x\nssh-copy-id: result=uncertain added=0\n",
        );
        assert_eq!(
            r,
            Report {
                outcome: Outcome::Uncertain,
                keys: vec![key(1, KeyStatus::Uncertain, b"x")],
            }
        );
    }

    #[test]
    fn b28_uncertain_summary_without_key_lines() {
        let r = parse_report(b"ssh-copy-id: result=uncertain added=0\n");
        assert_eq!(r.outcome, Outcome::Uncertain);
    }

    #[test]
    fn b29_uncertain_key_under_a_certain_summary_is_unknown() {
        for summary in ["installed", "partial", "unchanged"] {
            let stdout = format!(
                "ssh-copy-id: key=1 result=uncertain path=x\n\
                 ssh-copy-id: result={summary} added=0\n"
            );
            assert_eq!(
                parse_report(stdout.as_bytes()).outcome,
                Outcome::Unknown,
                "{summary}"
            );
        }
    }
}
