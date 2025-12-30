#!/bin/bash

cargo build --release --target x86_64-unknown-linux-musl

docker build -t executor-rust:1.0.0 .