# Command to run profiler
cargo build-sbf --manifest-path examples/escrow/Cargo.toml --debug --lto
cargo run --release -p quasar-profile -- target/sbpf-solana-solana/release/quasar_escrow.so -o profile.svg
