#!/usr/bin/env bash
# The oldest glibc the AppImage runs on (Ubuntu 24.04's): every ELF file under a folder is read for the glibc
# versions it needs, a weak need left out (the program runs without it), and one above the floor stops the package,
# naming the file and the version. Each file's highest need is printed, for the commit that measures it.
#
# Usage: scripts/package/glibc-floor.sh DIR
set -euo pipefail
GLIBC_FLOOR=2.39

dir=${1:?usage: glibc-floor.sh DIR}
over=0
while IFS= read -r -d '' file; do
  readelf -h "$file" > /dev/null 2>&1 || continue
  highest=$(readelf -V --wide "$file" 2>/dev/null |
    awk '/Name: GLIBC_[0-9.]+/ && !/Flags: WEAK/ { for (i = 1; i <= NF; i++) if ($i ~ /^GLIBC_[0-9.]+$/) print substr($i, 7) }' |
    sort -V | tail -n 1)
  [[ -n "$highest" ]] || continue
  echo "${file#"$dir"/}: glibc $highest"
  if [[ "$(printf '%s\n%s\n' "$highest" "$GLIBC_FLOOR" | sort -V | tail -n 1)" != "$GLIBC_FLOOR" ]]; then
    echo "FAIL: ${file#"$dir"/} needs glibc $highest, above the floor $GLIBC_FLOOR"
    over=1
  fi
done < <(find "$dir" -type f -print0 | sort -z)
exit "$over"
