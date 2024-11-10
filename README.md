## Prerequisites

### Rust
https://rustup.rs/

`rustup install`

### probe-rs
https://probe.rs/docs/getting-started/installation/

From binstall:

`cargo install cargo-binstall`

`cargo binstall probe-rs-tools`

From source:

`cargo install probe-rs-tools --locked`

### cargo-make

`cargo install cargo-make --locked`

## Upload

Calling cargo:
`cd clock` `cargo run`

Through cargo-make: `cargo make run`
