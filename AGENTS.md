See `README.md` for an overview of this Rust crate.

# Updating

When bumping the version number:
- adjust the version of **all** crates in `crates/`
- adjust the version of `aeronet_` dependencies in the root `Cargo.toml`
- run `cargo check --workspace`
- update the `Versions` table in `README.md`
- prompt the user to add a changelog entry to `crates/aeronet/docs/changelog.md`
