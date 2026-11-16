#!/bin/sh
# nextest setup script: the tests of a run get a scratch WireWeaver root and state directory and never fetch, so they
# neither read nor change what this machine has (~/.wire_weaver, ~/.local/state/ww).
dir=$(mktemp -d "${TMPDIR:-/tmp}/ww-test-home.XXXXXX")
echo "WIRE_WEAVER_HOME=$dir/wire_weaver" >> "$NEXTEST_ENV"
echo "WW_STATE_DIR=$dir/state" >> "$NEXTEST_ENV"
echo "WW_OFFLINE=1" >> "$NEXTEST_ENV"
