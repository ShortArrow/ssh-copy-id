//! The default key file: upstream's `DEFAULT_PUB_ID_FILE`.

use std::ffi::{OsStr, OsString};
use std::time::SystemTime;

/// A directory entry's name and modification time.
pub type DirEntryTime = (OsString, SystemTime);

/// The name, among the entries of `~/.ssh`, that upstream's
/// `ls -dt ~/.ssh/id*.pub | grep -v -- '-cert.pub$' | head -n 1` prints: names
/// that start with `id` and end in `.pub` but not in `-cert.pub`, the most
/// recently modified first, and among equally recent ones the first in byte
/// order, as `ls -t` breaks ties under `LC_ALL=C`. Entries of any kind count, as
/// `ls -d` lists directories too.
pub fn newest_public_key(entries: &[DirEntryTime]) -> Option<&OsStr> {
    entries
        .iter()
        .filter(|(name, _)| is_default_candidate(name))
        .min_by(|(a_name, a_time), (b_name, b_time)| {
            b_time
                .cmp(a_time)
                .then_with(|| a_name.as_encoded_bytes().cmp(b_name.as_encoded_bytes()))
        })
        .map(|(name, _)| name.as_os_str())
}

fn is_default_candidate(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    bytes.starts_with(b"id") && bytes.ends_with(b".pub") && !bytes.ends_with(b"-cert.pub")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn entries(list: &[(&str, u64)]) -> Vec<(OsString, SystemTime)> {
        list.iter()
            .map(|(name, secs)| {
                (
                    OsString::from(name),
                    UNIX_EPOCH + Duration::from_secs(*secs),
                )
            })
            .collect()
    }

    fn newest(list: &[(&str, u64)]) -> Option<String> {
        newest_public_key(&entries(list)).map(|name| name.to_string_lossy().into_owned())
    }

    #[test]
    fn d01_the_most_recently_modified_key_is_chosen() {
        assert_eq!(
            newest(&[
                ("id_rsa.pub", 10),
                ("id_ed25519.pub", 20),
                ("id_ecdsa.pub", 15)
            ]),
            Some("id_ed25519.pub".into())
        );
    }

    #[test]
    fn d02_a_certificate_is_never_chosen() {
        assert_eq!(
            newest(&[("id_ed25519.pub", 10), ("id_ed25519-cert.pub", 20)]),
            Some("id_ed25519.pub".into())
        );
        assert_eq!(newest(&[("id_ed25519-cert.pub", 20)]), None);
    }

    #[test]
    fn d03_only_names_matching_id_star_pub_count() {
        assert_eq!(
            newest(&[
                ("id_ed25519", 30),
                ("config", 30),
                ("my_id.pub", 30),
                ("id.pub.bak", 30),
                ("id.pub", 10),
            ]),
            Some("id.pub".into())
        );
    }

    #[test]
    fn d04_equally_recent_keys_are_ordered_by_name() {
        assert_eq!(
            newest(&[("id_rsa.pub", 20), ("id_ed25519.pub", 20), ("id_b.pub", 10)]),
            Some("id_ed25519.pub".into())
        );
    }

    #[test]
    fn d05_no_candidate_selects_nothing() {
        assert_eq!(newest(&[]), None);
        assert_eq!(newest(&[("known_hosts", 1)]), None);
    }

    #[test]
    fn d06_subsecond_differences_count() {
        let mut list = entries(&[("id_a.pub", 20), ("id_b.pub", 20)]);
        list[1].1 += Duration::from_micros(1);
        assert_eq!(newest_public_key(&list), Some(OsStr::new("id_b.pub")));
    }
}
