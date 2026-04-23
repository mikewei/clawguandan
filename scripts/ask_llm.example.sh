#!/usr/bin/env bash
# Example script for `clawguandan bot llm-bot --script ...`.
#
# Contract:
# - Read the full UTF-8 prompt from stdin (free text from llm-bot).
# - Write markers to stdout only (stderr may be used for logs).
#
# For local / CI testing without a model, this script always prints:
#   <<<DEFAULT>>>
# which means "use built-in default" (decision: suggest/pass/ready fallback;
# naming: fall back to bot0, bot1, ...). This token is intentionally NOT
# described in the prompts sent to a real LLM (see llm_bot/prompt.rs).
#
# Optional convention for your own script: if the prompt starts with a line
#   TASK=naming
# you may branch to emit <<<NAMING:LIST|{"names":[...]}>>> instead.
#
# chmod +x scripts/ask_llm.example.sh
set -euo pipefail
cat >/dev/null
printf '%s\n' '<<<DEFAULT>>>'
