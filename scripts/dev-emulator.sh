#!/usr/bin/env bash
# Start the AWS emulator container and run the dev server with the AWS SDK
# pointed at it.
#
# Usage: scripts/dev-emulator.sh [command...]   (default: kickstart up)
#
# Env:
#   AWS_EMULATOR_PORT   emulator port (default 9297)
#   AWS_EMULATOR_IMAGE  locally built image (default aws-emulator)
#   AWS_EMULATOR_AUTH   pass "off" to disable SigV4 checks (default: on)
#
# See https://docs.aws.amazon.com/emulator/latest/userguide/getting-started-docker.html
# and https://docs.aws.amazon.com/emulator/latest/userguide/point-sdk-to-emulator.html
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${AWS_EMULATOR_PORT:-9297}"
IMAGE="${AWS_EMULATOR_IMAGE:-aws-emulator}"
CONTAINER="bedrock-aws-emulator-${PORT}"
# Use 127.0.0.1, not localhost: some SDKs put bucket names in the host otherwise.
ENDPOINT="https://127.0.0.1:${PORT}"
EMU_DIR="${HOME}/.aws/emulator/${PORT}"
STARTED=""

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "Docker image '${IMAGE}' not found. Build it first:" >&2
  echo "  https://docs.aws.amazon.com/emulator/latest/userguide/getting-started-docker.html" >&2
  exit 1
fi

healthy() {
  [[ -f "${EMU_DIR}/ca.pem" ]] &&
    curl -sf --cacert "${EMU_DIR}/ca.pem" "${ENDPOINT}/__admin/health" >/dev/null 2>&1
}

running() {
  [[ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" == "true" ]]
}

cleanup() {
  if [[ -n "$STARTED" ]]; then
    echo "Stopping AWS emulator container ${CONTAINER}"
    docker stop "$CONTAINER" >/dev/null 2>&1 || true
    docker rm "$CONTAINER" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

if healthy; then
  echo "AWS emulator already running at ${ENDPOINT}; reusing it"
else
  # Without this, Docker creates the directory as root and the SDK can't read it.
  mkdir -p "${HOME}/.aws/emulator"
  # Clear a leftover stopped container with the same name.
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true

  emu_args=(-p "$PORT")
  [[ -n "${AWS_EMULATOR_AUTH:-}" ]] && emu_args+=(--auth "$AWS_EMULATOR_AUTH")

  echo "Starting AWS emulator container ${CONTAINER} on ${ENDPOINT}"
  docker run -d --name "$CONTAINER" \
    --user "$(id -u):$(id -g)" \
    --memory=2g --memory-swap=2g \
    -p "127.0.0.1:${PORT}:${PORT}" \
    -e HOME="$HOME" -v "${HOME}/.aws/emulator:${HOME}/.aws/emulator" \
    "$IMAGE" "${emu_args[@]}" >/dev/null
  STARTED=1

  for _ in $(seq 30); do
    healthy && break
    if ! running; then
      echo "AWS emulator container exited early. Last log lines:" >&2
      docker logs --tail 20 "$CONTAINER" >&2 || true
      exit 1
    fi
    sleep 1
  done
  if ! healthy; then
    echo "AWS emulator did not become healthy within 30s. Last log lines:" >&2
    docker logs --tail 20 "$CONTAINER" >&2 || true
    exit 1
  fi
  echo "AWS emulator is up (logs: docker logs -f ${CONTAINER})"
fi

# Real credentials make the emulator reject requests.
unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN AWS_PROFILE

export AWS_ENDPOINT_URL="$ENDPOINT"
export AWS_SHARED_CREDENTIALS_FILE="${EMU_DIR}/emulator_credentials"
export AWS_CONFIG_FILE="${EMU_DIR}/config"
export AWS_CA_BUNDLE="${EMU_DIR}/ca.pem"
export AWS_EC2_METADATA_DISABLED=true
export AWS_DISABLE_HOST_PREFIX_INJECTION=true
export AWS_REGION=us-east-1
export AWS_DEFAULT_REGION=us-east-1

# The Rust SDK trusts certs via SSL_CERT_FILE, which *replaces* the system trust
# store. Combine the system CAs with the emulator CA so cargo and anything else
# that talks to the real internet keeps working. The CA changes every emulator
# start, so rebuild it each run.
SYSTEM_CA=""
for f in /etc/ssl/certs/ca-certificates.crt /etc/pki/tls/certs/ca-bundle.crt /etc/ssl/cert.pem; do
  [[ -f "$f" ]] && SYSTEM_CA="$f" && break
done
COMBINED_CA="target/aws-emulator-ca-bundle.pem"
if [[ -n "$SYSTEM_CA" ]]; then
  cat "$SYSTEM_CA" "${EMU_DIR}/ca.pem" >"$COMBINED_CA"
else
  echo "warning: no system CA bundle found; trusting only the emulator CA" >&2
  cp "${EMU_DIR}/ca.pem" "$COMBINED_CA"
fi
export SSL_CERT_FILE="$PWD/$COMBINED_CA"

if [[ $# -eq 0 ]]; then
  set -- kickstart up
fi

echo "Running: $*"
"$@"
