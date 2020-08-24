#!/usr/bin/env bash

cd internet-radio-rs
cargo build --all-features
sudo setcap cap_net_bind_service=+ep target/debug/rradio
cargo run --all-features
