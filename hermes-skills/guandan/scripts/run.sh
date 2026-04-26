#!/usr/bin/env bash

set -euo pipefail

if command -v clawguandan >/dev/null 2>&1; then
  cmd="clawguandan"
elif [ -x "./node_modules/.bin/clawguandan" ]; then
  cmd="./node_modules/.bin/clawguandan"
else
  echo "ERROR: Unable to locate the 'clawguandan' executable." >&2
  echo "Searched locations:" >&2
  echo "  1) PATH (via command -v clawguandan)" >&2
  echo "  2) ./node_modules/.bin/clawguandan" >&2
  echo "Dependency setup is required before running this skill." >&2
  echo "Please follow the installation instructions in SKILL.md." >&2
  exit 1
fi

exec "$cmd" "$@"