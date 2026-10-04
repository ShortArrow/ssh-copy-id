# Pinned upstream ssh-copy-id

`ssh-copy-id` in this directory is test input for the P01 golden comparison
(`tests/p01_golden.rs`). It is not part of the product and is not packaged.

| Field | Value |
|---|---|
| Source | https://raw.githubusercontent.com/openssh/openssh-portable/eabf1987de772f0f2d772fd6bd72b4c84d0ab780/contrib/ssh-copy-id |
| Commit | `eabf1987de772f0f2d772fd6bd72b4c84d0ab780` |
| SHA-256 | `a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650` |
| License | BSD-2-Clause; the copyright holders and the notice are in the file itself |

The file is byte-identical to the source. `.gitattributes` keeps its LF line
endings, and the tests check its SHA-256 before running it. Moving to another
upstream commit means replacing the file and the digest in `tests/p01_golden.rs`
together.
