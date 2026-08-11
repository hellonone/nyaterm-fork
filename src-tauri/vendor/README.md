# Vendored RDP Dependencies

This directory contains local copies of upstream crates used by NyaTerm's RDP stack. Keep patches small and documented so the crates can be refreshed without turning them into long-lived forks.

## ironrdp-client

- Upstream: https://github.com/Devolutions/IronRDP
- Crate/version: `ironrdp-client` `0.1.0`
- Why vendored: NyaTerm needs runtime hooks that are not exposed by the published client API.
- NyaTerm changes:
  - Adds an injectable server certificate verifier called after TLS/RDCleanPath certificate extraction and before RDP finalize.
  - Adds an injectable CLIPRDR backend factory so NyaTerm can provide a text-only clipboard bridge instead of the native clipboard backend.
  - Exposes the clipboard module for non-Windows builds when the clipboard feature is enabled.

## ironrdp-connector

- Upstream: https://github.com/Devolutions/IronRDP
- Crate/version: `ironrdp-connector` `0.10.0`
- Why vendored: It must stay in lockstep with the vendored IronRDP client and the current connector API used by NyaTerm.
- NyaTerm changes: no intentional feature patches in this round; keep local edits limited to compatibility fixes required by the client.

## picky

- Upstream: https://github.com/Devolutions/picky-rs
- Crate/version: `picky` `7.0.0-rc.25`
- Why vendored: Required by IronRDP/SSPI auth dependencies and pinned to match the vendored connector stack.
- NyaTerm changes: no intentional RDP behavior patches in this round.

## sspi

- Upstream: https://github.com/Devolutions/sspi-rs
- Crate/version: `sspi` `0.21.0`
- Why vendored: CredSSP/NLA support must remain compatible with the vendored IronRDP connector and pinned `picky` version.
- NyaTerm changes: no intentional RDP behavior patches in this round.

## Update Method

1. Record the current NyaTerm patches with `git diff -- src-tauri/vendor/<crate>`.
2. Replace the target crate from the matching upstream tag or revision.
3. Reapply only the documented NyaTerm patches, preferring upstream APIs if they now exist.
4. Run `cargo update --manifest-path src-tauri/Cargo.toml -p <crate>` if dependency metadata changed.
5. Run `cargo fmt`, `cargo test`, and `cargo clippy` for `src-tauri/Cargo.toml`.
6. Update this README with the new version/revision and any changed local patches.
