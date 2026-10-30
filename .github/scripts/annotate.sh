#!/usr/bin/env bash
# Runs a command. If it fails, its error lines are repeated as workflow annotations so they can be read
# without downloading the whole log.
log=$(mktemp)
"$@" > "$log" 2>&1
code=$?
cat "$log"
if [ $code -ne 0 ]; then
  grep -v "^::" "$log" | grep -E "error|Error|FAIL|panicked|^\s*-->|\.svelte|\.ts:|assert" | head -60 | cut -c1-400 | while IFS= read -r l; do echo "::error::$l"; done
fi
exit $code
