#!/usr/bin/env bash -x

##cd internet-radio-rs
cargo build --release --all-features
sudo setcap cap_net_bind_service=+ep target/release/rradio
target/release/rradio
