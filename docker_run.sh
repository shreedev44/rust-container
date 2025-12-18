#!/bin/bash

docker run \
  --read-only \
  --rm \
  -p 8000:8000 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=128m \
  --memory=256m \
  --pids-limit=64 \
  --cpus=0.5 \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  executor-js

