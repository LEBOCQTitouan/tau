#!/usr/bin/env bash
# Re-record OpenAI cassettes against the live API.
#
# Costs ~$0.005 per full re-record cycle on gpt-4o-mini.
#
# Usage:
#   OPENAI_API_KEY=sk-proj-... ./scripts/rerecord-openai-cassettes.sh
#
# Optional environment:
#   TAU_OPENAI_LIVE_MODEL=gpt-4o    # override default gpt-4o-mini
#
# v0.1: cassettes are hand-authored. This script doesn't yet automate
# regeneration. Instead, it points to the live test suite as a way to
# verify that the cassette responses still match what OpenAI emits.
# When automated cassette regeneration lands (a future sub-project),
# this script will set TAU_RECORD_CASSETTES=1 and run the cassette-
# aware tests with record mode enabled.

set -euo pipefail

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
    echo "error: OPENAI_API_KEY environment variable is required" >&2
    exit 1
fi

MODEL="${TAU_OPENAI_LIVE_MODEL:-gpt-4o-mini}"

cat <<EOF
Cassette files live at:
  crates/tau-plugins/openai/tests/cassettes/

v0.1 cassettes are hand-authored. To verify the live OpenAI API still
matches the canned responses, the live test suite is the drift-
detection mechanism:

  TAU_OPENAI_LIVE_TESTS=1 \\
  TAU_OPENAI_LIVE_MODEL="$MODEL" \\
  OPENAI_API_KEY=\$OPENAI_API_KEY \\
    cargo test -p openai --test live -- --ignored --nocapture

If the live response shape diverges from the cassettes, manually
update the YAML files under tests/cassettes/.

Future work: automated record-mode replayer (set
TAU_RECORD_CASSETTES=1 and run the regular cassette tests; the
replayer captures real responses + writes them back to the YAMLs).

Running live tests now...
EOF

TAU_OPENAI_LIVE_TESTS=1 \
TAU_OPENAI_LIVE_MODEL="$MODEL" \
    cargo test -p openai --test live -- --ignored --nocapture
