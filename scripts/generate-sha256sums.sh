#!/usr/bin/env bash
set -euo pipefail

directory="${1:?usage: generate-sha256sums.sh <directory>}"
output="$directory/SHA256SUMS"
temp="$output.tmp"

(
  cd "$directory"
  for file in *; do
    [[ "$file" == SHA256SUMS || "$file" == SHA256SUMS.tmp || -f "$file" ]] || continue
    [[ -f "$file" ]] || continue
    printf '%s\n' "$file"
  done | LC_ALL=C sort | while IFS= read -r file; do
    sha256sum "$file"
  done > "$temp"
)

mv "$temp" "$output"
