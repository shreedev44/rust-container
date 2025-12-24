#!/bin/bash

docker run \
  --read-only \
  --rm \
  -p 8000:8000 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=32m \
  --tmpfs /work:rw,nosuid,nodev,exec,size=32m \
  --memory=64m \
  --pids-limit=64 \
  --cpus=0.5 \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  executor-rust

