#!/usr/bin/env bash
# Verify the live Ollama API still matches the hand-authored cassettes.
#
# v0.1: cassettes are hand-authored. This script doesn't yet automate
# regeneration. Instead, it ensures Ollama is installed and running,
# and runs the live test suite against the local instance.
# When automated cassette regeneration lands (a future sub-project),
# this script will set TAU_RECORD_CASSETTES=1 and run the cassette-
# aware tests with record mode enabled.
#
# Usage:
#   ./scripts/rerecord-ollama-cassettes.sh
#
# Optional environment:
#   TAU_OLLAMA_LIVE_MODEL=llama3.1   # override default llama3.2

set -euo pipefail

if ! command -v ollama >/dev/null 2>&1; then
    echo "error: ollama CLI not found" >&2
    echo "Install via 'brew install ollama' or https://ollama.com/download." >&2
    exit 1
fi

MODEL="${TAU_OLLAMA_LIVE_MODEL:-llama3.2}"

echo "Pulling model: $MODEL"
ollama pull "$MODEL"

cat <<EOF
Cassette files live at:
  crates/tau-plugins/ollama/tests/cassettes/

v0.1 cassettes are hand-authored. To verify the live Ollama API still
matches the canned responses, the live test suite is the drift-
detection mechanism:

  TAU_OLLAMA_LIVE_TESTS=1 \\
  TAU_OLLAMA_LIVE_MODEL="$MODEL" \\
    cargo test -p ollama --test live -- --ignored --nocapture

If the live response shape diverges from the cassettes, manually
update the YAML files under tests/cassettes/.

Future work: automated record-mode replayer (set
TAU_RECORD_CASSETTES=1 and run the regular cassette tests; the
replayer captures real responses + writes them back to the YAMLs).

Running live tests now...
EOF

TAU_OLLAMA_LIVE_TESTS=1 \
TAU_OLLAMA_LIVE_MODEL="$MODEL" \
    cargo test -p ollama --test live -- --ignored --nocapture
