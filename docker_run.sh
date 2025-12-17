#!/bin/bash

docker run \
  --rm \
  -p 8000:8000 \
  --tmpfs /tmp:rw,noexec,nosuid,size=64m \
  --memory=256m \
  --pids-limit=64 \
  --cpus=0.5 \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  executor

