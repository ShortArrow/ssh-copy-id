//! The `-x` trace of a client command: the line printed before it runs.

use crate::remote_script::sh_quote;

/// The trace line for running `program` with `args`, without a line end: `+ `,
/// then each word as bash's `set -x` prints it. A word stays bare unless it is
/// empty or holds a character that bash quotes in its trace; such a word is
/// single-quoted with each `'` written as `'\''`. Characters bash quotes are
/// whitespace, control characters, `'"\|&;()<>!{}*[?]^$` and backquote, `#` as
/// the first character, and `~` as the first character or after `=` or `:`.
pub fn command_line(program: &str, args: &[String]) -> String {
    let words: Vec<String> = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(traced_word)
        .collect();
    format!("+ {}", words.join(" "))
}

fn traced_word(word: &str) -> String {
    if needs_quoting(word) {
        sh_quote(word)
    } else {
        word.to_string()
    }
}

fn needs_quoting(word: &str) -> bool {
    let mut previous = None;
    word.is_empty()
        || word.chars().any(|c| {
            let quoted = match c {
                '#' => previous.is_none(),
                '~' => matches!(previous, None | Some('=' | ':')),
                _ => c.is_whitespace() || c.is_control() || "'\"\\|&;()<>!{}*[?]^$`".contains(c),
            };
            previous = Some(c);
            quoted
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(program: &str, args: &[&str]) -> String {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        command_line(program, &args)
    }

    #[test]
    fn t01_plain_words_stay_bare() {
        assert_eq!(
            line(
                "ssh",
                &[
                    "-a",
                    "-x",
                    "-o",
                    "ControlPath=none",
                    "-p",
                    "22",
                    "u@h.example",
                    "C:/k/id_1.pub",
                    "a%b,c+d"
                ]
            ),
            "+ ssh -a -x -o ControlPath=none -p 22 u@h.example C:/k/id_1.pub a%b,c+d"
        );
        assert_eq!(line("ssh-add", &["-L"]), "+ ssh-add -L");
    }

    #[test]
    fn t02_words_with_shell_syntax_are_single_quoted() {
        for (word, expected) in [
            ("a b", "'a b'"),
            ("a\tb", "'a\tb'"),
            ("a\nb", "'a\nb'"),
            ("$HOME", "'$HOME'"),
            ("C:\\k\\id", "'C:\\k\\id'"),
            ("a\"b", "'a\"b'"),
            ("x;y", "'x;y'"),
            ("*", "'*'"),
            ("`id`", "'`id`'"),
            ("a!b", "'a!b'"),
            ("a\u{1}b", "'a\u{1}b'"),
        ] {
            assert_eq!(
                line("ssh", &[word]),
                format!("+ ssh {expected}"),
                "{word:?}"
            );
        }
    }

    #[test]
    fn t03_a_single_quote_is_written_as_quote_backslash_quote_quote() {
        assert_eq!(line("ssh", &["it's"]), r"+ ssh 'it'\''s'");
    }

    #[test]
    fn t04_an_empty_word_is_two_quotes() {
        assert_eq!(line("ssh", &["", "h"]), "+ ssh '' h");
    }

    #[test]
    fn t05_non_ascii_text_stays_bare() {
        assert_eq!(line("ssh", &["名前@ホスト"]), "+ ssh 名前@ホスト");
    }

    #[test]
    fn t06_hash_and_tilde_are_quoted_only_where_the_shell_expands_them() {
        for (word, expected) in [
            ("#x", "'#x'"),
            ("x#", "x#"),
            ("~/k", "'~/k'"),
            ("a=~/k", "'a=~/k'"),
            ("a:~/k", "'a:~/k'"),
            ("a~b", "a~b"),
        ] {
            assert_eq!(
                line("ssh", &[word]),
                format!("+ ssh {expected}"),
                "{word:?}"
            );
        }
    }
}
