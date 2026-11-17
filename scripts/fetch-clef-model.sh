#!/usr/bin/env bash
# Copyright 2026 LiteRT-LM Rust Contributors
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

# Provision or link the CLEF Gemma-4-E2B-it LiteRT-LM model (`gemma-4-E2B-it.litertlm`)
# into `<repo>/gemma-4-E2B-it.litertlm` from `$HOME/.cache/litert-lm`,
# Outrider's public S3 bucket (`s3://outrider-public-models/clef/clef-flash.litertlm`),
# or the SHA-256-pinned `litert-community/gemma-4-E2B-it-litert-lm` mirror.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="${LITERT_LM_RUST_DIR:-$(cd "$SCRIPT_DIR/.." && pwd)}"
OUTRIDER_S3_URL="${OUTRIDER_CLEF_MODEL_URL:-https://outrider-public-models.s3.eu-central-1.amazonaws.com/clef/clef-flash.litertlm}"
HF_FALLBACK_URL="https://huggingface.co/litert-community/gemma-4-E2B-it-litert-lm/resolve/main/gemma-4-E2B-it.litertlm"
EXPECTED_SHA="181938105e0eefd105961417e8da75903eacda102c4fce9ce90f50b97139a63c"
EXPECTED_SIZE="2588147712"
LINK_ONLY=0
VERIFY_SHA=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO_DIR="$2"
      shift 2
      ;;
    --link-if-cached)
      LINK_ONLY=1
      shift
      ;;
    --verify-sha256)
      VERIFY_SHA=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/fetch-clef-model.sh [--repo <dir>] [--link-if-cached] [--verify-sha256]

Links or downloads `gemma-4-E2B-it.litertlm` into `<repo>/gemma-4-E2B-it.litertlm`
using `$HOME/.cache/litert-lm`, Outrider's public S3 bucket
(`https://outrider-public-models.s3.eu-central-1.amazonaws.com/clef/clef-flash.litertlm`),
or the SHA-256-pinned `litert-community/gemma-4-E2B-it-litert-lm` fallback.
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

MODEL_DIR="$HOME/.cache/litert-lm"
mkdir -p "$MODEL_DIR"
MODEL_CACHE="$MODEL_DIR/gemma-4-E2B-it.litertlm"

have_valid_cache() {
  [[ -f "$MODEL_CACHE" ]] || return 1
  local size
  size="$(stat -c '%s' "$MODEL_CACHE" 2>/dev/null || stat -f '%z' "$MODEL_CACHE" 2>/dev/null || echo 0)"
  if [[ "$VERIFY_SHA" -eq 1 ]]; then
    [[ "$(sha256sum "$MODEL_CACHE" | awk '{print $1}')" == "$EXPECTED_SHA" ]]
  else
    [[ "$size" == "$EXPECTED_SIZE" ]]
  fi
}

if ! have_valid_cache; then
  if [[ "$LINK_ONLY" -eq 1 ]]; then
    echo "[clef-model] Cached model not present at $MODEL_CACHE (run 'scripts/fetch-clef-model.sh' to download)."
    exit 0
  fi
  PART_FILE="${MODEL_CACHE}.part"
  SIDECAR_SHA=""
  if SIDECAR_TEXT="$(curl -fsSL --max-time 10 "${OUTRIDER_S3_URL}.sha256" 2>/dev/null)" && [[ -n "$SIDECAR_TEXT" ]]; then
    SIDECAR_SHA="$(awk '{print tolower($1)}' <<<"$SIDECAR_TEXT")"
    echo "[clef-model] Downloading CLEF model from Outrider S3 ($OUTRIDER_S3_URL)..."
    if curl -fSL --retry 3 "$OUTRIDER_S3_URL" -o "$PART_FILE"; then
      ACTUAL_SHA="$(sha256sum "$PART_FILE" | awk '{print $1}')"
      if [[ "${#SIDECAR_SHA}" -eq 64 && "$ACTUAL_SHA" == "$SIDECAR_SHA" ]]; then
        mv -f "$PART_FILE" "$MODEL_CACHE"
      else
        rm -f "$PART_FILE"
      fi
    fi
  fi
  if [[ ! -f "$MODEL_CACHE" ]]; then
    echo "[clef-model] Outrider S3 unavailable/403; fetching SHA-256-pinned gemma-4-E2B-it.litertlm ($EXPECTED_SHA)..."
    curl -fSL --retry 3 "$HF_FALLBACK_URL" -o "$PART_FILE"
    ACTUAL_SHA="$(sha256sum "$PART_FILE" | awk '{print $1}')"
    if [[ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]]; then
      rm -f "$PART_FILE"
      echo "[clef-model] SHA-256 mismatch: got $ACTUAL_SHA, expected $EXPECTED_SHA" >&2
      exit 1
    fi
    mv -f "$PART_FILE" "$MODEL_CACHE"
    printf '%s  gemma-4-E2B-it.litertlm\n' "$EXPECTED_SHA" > "${MODEL_CACHE}.sha256"
  fi
fi

if [[ -d "$REPO_DIR" ]]; then
  ln -sfn "$MODEL_CACHE" "$REPO_DIR/gemma-4-E2B-it.litertlm"
  echo "[clef-model] Linked $REPO_DIR/gemma-4-E2B-it.litertlm -> $MODEL_CACHE"
fi
