[parallel]
check: \
    check-typos check-tombi check-fmt check-shear check-fuzz \
    (check-clippy "x86_64-unknown-linux-gnu") (check-clippy "wasm32-unknown-unknown") \
    (check-doc "x86_64-unknown-linux-gnu") (check-doc "wasm32-unknown-unknown")

check-typos:
    typos

check-tombi:
    tombi lint
    tombi fmt --check

check-fmt:
    cargo +nightly fmt --check

check-shear:
    cargo shear

check-fuzz:
    cd crates/aeronet_transport && cargo +nightly fuzz check --target x86_64-unknown-linux-gnu

check-clippy target:
    cargo clippy --target {{ target }} --workspace --all-features --all-targets -- -Dwarnings

check-doc target:
    cargo +nightly doc --target {{ target }} --workspace --all-features --no-deps

prepare:
    typos --write-changes
    tombi fmt
    cargo +nightly fmt
    cargo shear --fix
    cargo clippy --fix --allow-dirty
