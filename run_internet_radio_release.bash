#!/usr/bin/env bash

##cd internet-radio-rs
cargo build --release --all-features
sudo setcap cap_net_bind_service=+ep target/release/rradio
cargo run --release --all-features
