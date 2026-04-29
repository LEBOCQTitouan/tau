#!/usr/bin/env bash
# Re-record Anthropic cassettes against the live API.
#
# Costs ~$0.05 per full re-record cycle.
#
# Usage:
#   ANTHROPIC_API_KEY=sk-ant-... ./scripts/rerecord-anthropic-cassettes.sh
#
# v0.1: cassettes are hand-authored. This script doesn't yet automate
# regeneration. Instead, it points to the live test suite as a way to
# verify that the cassette responses still match what Anthropic emits.
# When automated cassette regeneration lands (a future sub-project),
# this script will set TAU_RECORD_CASSETTES=1 and run the cassette-aware
# tests with record mode enabled.

set -euo pipefail

if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
    echo "error: ANTHROPIC_API_KEY environment variable is required" >&2
    exit 1
fi

cat <<'EOF'
Cassette files live at:
  crates/tau-plugins/anthropic/tests/cassettes/

v0.1 cassettes are hand-authored. To verify the live API still matches
the canned responses, run the live test suite:

  TAU_ANTHROPIC_LIVE_TESTS=1 ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY \
    cargo test -p anthropic --test live -- --ignored --nocapture

If the live response shape diverges from the cassettes, manually
update the YAML files under tests/cassettes/.

Future work: automated record-mode replayer (set TAU_RECORD_CASSETTES=1
and run the regular cassette tests; the replayer captures real
responses + writes them back to the YAMLs).
EOF
