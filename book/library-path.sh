#!/bin/sh
# The search path `mdbook test -L` needs: every directory under the book's
# target directory that holds a compiled crate, comma-separated.
#
# `mdbook test` gives rustdoc `-L` and no `--extern`, so `extern crate guido`
# is found by searching, and so is every crate guido depends on. Cargo before
# 1.100 put them all in `debug/deps`; from 1.100 each has a directory of its
# own under `debug/build/<crate>/<hash>/out`, and only guido is copied up into
# `debug`. Listing whatever holds a crate works for both.
set -eu
dir=${1:-target/book/debug}
path=$(find "$dir" \( -name '*.rlib' -o -name '*.so' \) -printf '%h\n' | sort -u | paste -sd, -)
if [ -z "$path" ]; then
    echo "no compiled crate under $dir: run cargo build --all-features --target-dir target/book first" >&2
    exit 1
fi
echo "$path"
