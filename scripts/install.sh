#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

gitsync stop
cargo build --release
sudo cp target/release/gitsync /usr/local/bin
gitsync start -b

