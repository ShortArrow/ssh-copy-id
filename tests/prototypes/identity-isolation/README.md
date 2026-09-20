# Selected-Identity Experiment

This is a disposable experiment, not production configuration-handling code.
It compares the pinned upstream script against a generated SSH configuration
with only the selected identity. No Rust dependencies are added.

From the repository root:

```sh
docker build -t ssh-copy-id-identity-probe:local tests/prototypes/identity-isolation
docker run --rm --network none ssh-copy-id-identity-probe:local
```

Building downloads the declared Ubuntu packages and pinned upstream script.
Running requires no external network, published ports, mounted directories, or
host SSH configuration. Keys and two localhost SSH servers exist only inside the
disposable container. The fixture account has public-key authentication only.

The target initially accepts A, while B is selected for installation. The client
configuration contains A. A separate jump server accepts a third identity.
The script checks upstream skipping, effective identities, rejection of absent B,
success with installed B, and host-key verification for direct and jump paths.
It also checks server logs for authentication fingerprints and prints JSON results.

The alternative uses `ssh -G` to evaluate configuration, removes identity and
certificate lists, and writes a temporary `-F` configuration. The jump connection
uses the original configuration through a separate `ssh -W` process. This prototype
supports only the fixed fixture paths and one jump alias. It disables target-agent
use to isolate the file-key case; this is not the product's agent policy.

Do not generalize this serialization to arbitrary user configuration without
testing quoting, percent expansion, Match conditions, multiple jumps, certificates,
agents, connection multiplexing, and OpenSSH version differences. This experiment
uses Linux OpenSSH clients and servers; it does not validate Windows client
argument handling or interactive authentication.
