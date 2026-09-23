See `README.md` for an overview of this Rust crate.

# Checking

Use `just prepare` to apply any automatic fixes like typos, formatting. Use `just check` to run the same checks as CI. Always use `cargo clippy` instead of `cargo check`.

# Updating

When bumping the version number:
- adjust the version of **all** crates in `crates/`
- adjust the version of `aeronet_` dependencies in the root `Cargo.toml`
- run `cargo check --workspace`
- update the `Versions` table in `README.md`
- prompt the user to add a changelog entry to `crates/aeronet/docs/changelog.md`

# Nags

General project-specific guidance.
- When writing tests, place simpler tests at the top and more complicated tests at the bottom.
