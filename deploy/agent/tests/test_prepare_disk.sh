#!/usr/bin/env bash
# Tests for skimasque-prepare-disk, with blkid/mkfs.ext4/mount/mountpoint stubbed
# so nothing real is formatted or mounted. Run on Linux: bash tests/test_prepare_disk.sh
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
script="$here/../skimasque-prepare-disk"
fail=0

run_case() { # name blkid_status expect_mkfs(0|1) expect_exit [device_content]
  local name=$1 blkid_status=$2 expect_mkfs=$3 expect_exit=$4 device_content=${5:-}
  local tmp
  tmp=$(mktemp -d)
  mkdir -p "$tmp/bin"
  cat >"$tmp/bin/blkid" <<EOF
#!/bin/sh
exit $blkid_status
EOF
  for cmd in mkfs.ext4 mount; do
    cat >"$tmp/bin/$cmd" <<EOF
#!/bin/sh
echo "$cmd \$*" >>"$tmp/calls"
EOF
  done
  # mountpoint reports "not mounted" until mount has been called.
  cat >"$tmp/bin/mountpoint" <<EOF
#!/bin/sh
grep -qs '^mount ' "$tmp/calls"
EOF
  chmod +x "$tmp"/bin/*
  : >"$tmp/calls"
  printf '%s' "$device_content" >"$tmp/device"
  : >"$tmp/fstab"

  PATH="$tmp/bin:$PATH" FSTAB="$tmp/fstab" bash "$script" "$tmp/device" "$tmp/mnt" skimasque-state >"$tmp/out" 2>&1
  local got_exit=$?
  local got_mkfs=0
  grep -q '^mkfs.ext4' "$tmp/calls" && got_mkfs=1

  if [ "$got_exit" -ne "$expect_exit" ] || [ "$got_mkfs" -ne "$expect_mkfs" ]; then
    echo "FAIL $name: exit=$got_exit (want $expect_exit) mkfs=$got_mkfs (want $expect_mkfs)"
    sed 's/^/    /' "$tmp/out"
    fail=1
  else
    echo "ok   $name"
  fi
  # Extra checks for the success paths.
  if [ "$expect_exit" -eq 0 ]; then
    if ! grep -q "^LABEL=skimasque-state $tmp/mnt ext4 defaults,nofail 0 2\$" "$tmp/fstab"; then
      echo "FAIL $name: fstab entry missing"; fail=1
    fi
    if ! grep -q '^mount ' "$tmp/calls"; then
      echo "FAIL $name: not mounted"; fail=1
    fi
  fi
  rm -rf "$tmp"
}

run_case "a blank disk is formatted and mounted" 2 1 0
run_case "a disk with a filesystem is never formatted" 0 0 0
run_case "an unexpected blkid failure aborts without formatting" 4 0 1
run_case "blkid says blank but the disk holds data: refuse to format" 2 0 1 "not-zeros-this-is-somebodys-data"

# Idempotent: a second run adds no second fstab line.
tmp=$(mktemp -d); mkdir -p "$tmp/bin"
printf '#!/bin/sh\nexit 0\n' >"$tmp/bin/blkid"
printf '#!/bin/sh\necho "mount $*" >>"%s/calls"\n' "$tmp" >"$tmp/bin/mount"
printf '#!/bin/sh\ngrep -qs "^mount " "%s/calls"\n' "$tmp" >"$tmp/bin/mountpoint"
chmod +x "$tmp"/bin/*; : >"$tmp/calls"; : >"$tmp/device"; : >"$tmp/fstab"
for _ in 1 2; do
  PATH="$tmp/bin:$PATH" FSTAB="$tmp/fstab" bash "$script" "$tmp/device" "$tmp/mnt" skimasque-state >/dev/null 2>&1
done
lines=$(grep -c '^LABEL=skimasque-state ' "$tmp/fstab")
mounts=$(grep -c '^mount ' "$tmp/calls")
if [ "$lines" -eq 1 ] && [ "$mounts" -eq 1 ]; then echo "ok   idempotent: one fstab line, one mount"; else echo "FAIL idempotent: fstab=$lines mounts=$mounts"; fail=1; fi
rm -rf "$tmp"

# A missing device is an error.
if bash "$script" /nonexistent/device /tmp/x lbl >/dev/null 2>&1; then echo "FAIL missing device accepted"; fail=1; else echo "ok   a missing device is an error"; fi
# Wrong arity is a usage error.
if bash "$script" one two >/dev/null 2>&1; then echo "FAIL bad usage accepted"; fail=1; else echo "ok   wrong arguments are a usage error"; fi

exit "$fail"
