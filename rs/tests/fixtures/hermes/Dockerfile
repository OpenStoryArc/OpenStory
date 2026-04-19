# Slim Hermes test container for OpenStory translator verification.
#
# Produces a session log (both snapshot JSON and plugin JSONL) using
# hermes-agent's canonical message shapes. No API key needed.
#
# Build (from the fixture directory as context):
#   docker build -t hermes-fixture:test rs/tests/fixtures/hermes/
#
# Run:
#   docker run --rm -v /tmp/hermes-output:/output hermes-fixture:test
#
# Or use testcontainers in Rust — see rs/tests/test_hermes_translator.rs.

FROM python:3.11-slim

# The fixture script only needs stdlib (json, datetime, uuid, pathlib).
# No hermes-agent install needed — the shapes are constructed directly
# from the verified specifications in SOURCE_VERIFICATION.md §4.

COPY generate_session.py /fixture/generate_session.py

RUN mkdir -p /output
ENV OUTPUT_DIR=/output

ENTRYPOINT ["python3", "/fixture/generate_session.py"]
