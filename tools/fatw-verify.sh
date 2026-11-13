#!/usr/bin/env bash
# Runs inside WSL. Usage: bash tools/fatw-verify.sh <dir with out-*.img and expect-*.txt>
# fsck.vfat must call every image clean, and its list of files must equal what the test expects.
cd "$1" || exit 2
fail=0
for img in out-*.img; do
  tag=${img#out-}; tag=${tag%.img}
  rc=0; fsck.vfat -n -v "$img" > "fsck-$tag.txt" 2>&1 || rc=$?
  # lines look like "Checking file /docs/Long name.txt (LONGNA~1.TXT)"; the root and the volume label are not files
  fsck.vfat -n -l "$img" 2>&1 | sed -n 's/^Checking file //p' | sed 's/ ([^()]*)$//' | grep -v -x -e '/' -e '/AEON' | sort > "list-$tag.txt" || true
  if [ "$rc" -ne 0 ]; then echo "FAIL $tag: fsck.vfat exit $rc"; sed -n 1,25p "fsck-$tag.txt"; fail=1; else echo "PASS $tag: fsck.vfat clean"; fi
  sort "expect-$tag.txt" > "expect-sorted-$tag.txt"
  if diff -i "list-$tag.txt" "expect-sorted-$tag.txt" > "diff-$tag.txt"; then echo "PASS $tag: fsck file list matches ($(wc -l < "list-$tag.txt") paths)"; else echo "FAIL $tag: file list differs"; head -20 "diff-$tag.txt"; fail=1; fi
done
exit $fail
