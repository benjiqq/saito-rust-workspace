#!/bin/bash
#RUST_LOG=debug cargo run -- --config configs/config.json
RUST_LOG=debug cargo run -- --config configs/config.json | tee saito.log

