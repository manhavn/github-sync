#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

cargo build --release

gitsync stop
sudo gitsync stop
sudo cp target/release/gitsync /usr/local/bin
gitsync start -b

