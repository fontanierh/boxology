# example

This generated Boxology project contains the `ping` box and the `ping-app` composition, with the same capability bound in-process and over HTTP.

## Build

```sh
cargo build --workspace
```

This first ordinary Cargo build creates the derived `Cargo.lock`; the initializer deliberately does not emit it.

## Invoke through Rust and HTTP

```sh
cargo test -p ping-app assembled_ping_answers_in_process_and_over_real_http
```

The test starts the composition, invokes `ping.ping` through its Rust binding, then sends a real HTTP request to `/rpc/ping/ping`.

## Validate

```sh
boxology check
```
