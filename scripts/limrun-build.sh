#!/usr/bin/env bash
# Copyright 2026 LiteRT-LM Rust Contributors
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

# Build or hydrate upstream `google-ai-edge/LiteRT-LM` (`//c:litert-lm`) using
# Limrun 96-vCPU sandboxes, Limrun Asset Storage, or the SLSA-attested
# public GitHub Release fallback (Linux x86_64 and macOS arm64).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="${LITERT_LM_RUST_DIR:-$(cd "$SCRIPT_DIR/.." && pwd)}"
UPSTREAM_REPO="https://github.com/google-ai-edge/LiteRT-LM.git"
DEFAULT_UPSTREAM_REF="083c49c4a088ba7675186292f43c92d8f5464056"
PINNED_REF="083c49c4a088ba7675186292f43c92d8f5464056"
FORCE_REBUILD=0
HYDRATE_ONLY=0
KEEP_SANDBOX=0
IF_AUTHENTICATED=0
CUSTOM_REF=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO_DIR="$2"
      shift 2
      ;;
    --ref)
      CUSTOM_REF="$2"
      shift 2
      ;;
    --force-rebuild)
      FORCE_REBUILD=1
      shift
      ;;
    --hydrate-only)
      HYDRATE_ONLY=1
      shift
      ;;
    --keep-sandbox)
      KEEP_SANDBOX=1
      shift
      ;;
    --if-authenticated)
      IF_AUTHENTICATED=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/limrun-build.sh [--repo <dir>] [--ref <sha>] [--force-rebuild] [--hydrate-only] [--if-authenticated] [--keep-sandbox]

Hydrates `prebuilt/` (`liblitert-lm.{so,dylib}` and companion runtime libraries)
from Limrun Asset Storage or the SHA-256-verified public GitHub Release, or builds
`google-ai-edge/LiteRT-LM` (`//c:litert-lm`) on a 96-vCPU Limrun sandbox and
pushes the artifact to Limrun Asset Storage.
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/bin:$PATH"

sha256_hex() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print tolower($1)}'
  else
    shasum -a 256 "$1" | awk '{print tolower($1)}'
  fi
}

ensure_lim_auth() {
  if [[ -n "${LIM_API_KEY:-}" ]]; then
    return 0
  fi
  if [[ -s "$HOME/.gemini/lim/config.yaml" && ! -s "$HOME/.lim/config.yaml" ]]; then
    mkdir -p "$HOME/.lim"
    ln -sfn "$HOME/.gemini/lim/config.yaml" "$HOME/.lim/config.yaml"
  fi
  if [[ -s "$HOME/.lim/config.yaml" ]]; then
    return 0
  fi
  for env_file in "$REPO_DIR/.limrun.env" "$HOME/.config/limrun/env"; do
    if [[ -f "$env_file" ]]; then
      local key
      key="$(grep -E '^(export[[:space:]]+)?LIM_API_KEY=' "$env_file" | tail -n1 | cut -d= -f2- | tr -d '"' | tr -d "'" | tr -d '\r' || true)"
      if [[ -n "$key" ]]; then
        export LIM_API_KEY="$key"
        return 0
      fi
    fi
  done
  if [[ "$IF_AUTHENTICATED" -eq 1 ]]; then
    echo "[limrun-litert-lm] Skipping Limrun sandbox build (no Limrun credentials configured)."
    exit 0
  fi
  echo "limrun-build.sh: No Limrun API key configured (run 'lim login' or set LIM_API_KEY)." >&2
  return 1
}

if [[ -n "$CUSTOM_REF" ]]; then
  UPSTREAM_REF="$CUSTOM_REF"
elif [[ -f "$REPO_DIR/c/UPSTREAM_COMMIT" ]]; then
  UPSTREAM_REF="$(tr -d '[:space:]' < "$REPO_DIR/c/UPSTREAM_COMMIT")"
else
  UPSTREAM_REF="$DEFAULT_UPSTREAM_REF"
fi

HOST_OS="$(uname -s)"
HOST_ARCH="$(uname -m)"
case "${HOST_OS}/${HOST_ARCH}" in
  Linux/x86_64)
    PLATFORM_SLUG="linux-x86_64"
    RUST_TARGET="x86_64-unknown-linux-gnu"
    MAIN_LIB="liblitert-lm.so"
    ;;
  Darwin/arm64)
    PLATFORM_SLUG="macos-arm64"
    RUST_TARGET="aarch64-apple-darwin"
    MAIN_LIB="liblitert-lm.dylib"
    ;;
  *)
    echo "limrun-build.sh: Unsupported host ${HOST_OS}/${HOST_ARCH} (use Cargo download-native on Windows)." >&2
    exit 1
    ;;
esac

CRATE_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO_DIR/Cargo.toml" | head -n1)"
CRATE_VERSION="${CRATE_VERSION:-0.3.1}"
PUBLIC_RELEASE_URL="https://github.com/maceip/litert-lm-rust/releases/download/v${CRATE_VERSION}/litert-lm-v${CRATE_VERSION}-${RUST_TARGET}.tar.gz"

SHORT_REF="${UPSTREAM_REF:0:12}"
ASSET_NAME="litert-lm-${PLATFORM_SLUG}-${SHORT_REF}.tar.gz"
SHA_ASSET_NAME="${ASSET_NAME}.sha256"
PREBUILT_DIR="$REPO_DIR/prebuilt"
STAMP_FILE="$PREBUILT_DIR/.limrun-stamp"

if [[ "$FORCE_REBUILD" -eq 0 && -f "$PREBUILT_DIR/$MAIN_LIB" && -f "$STAMP_FILE" ]]; then
  if [[ "$(cat "$STAMP_FILE")" == "$ASSET_NAME" ]]; then
    if [[ "$HOST_OS" != "Linux" ]] || readelf -d "$PREBUILT_DIR/liblitert-lm.so" 2>/dev/null | grep -E 'RPATH|RUNPATH' | grep -q '\$ORIGIN'; then
      echo "[limrun-litert-lm] Already hydrated ($ASSET_NAME) in $PREBUILT_DIR"
      exit 0
    fi
  fi
fi

TMP_WORK="$(mktemp -d "${TMPDIR:-/tmp}/limrun-litert.XXXXXX")"
cleanup() {
  rm -rf "$TMP_WORK"
  if [[ "$KEEP_SANDBOX" -eq 0 && -n "${SANDBOX_ID:-}" ]]; then
    lim gradle delete "$SANDBOX_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

verify_sidecar_file() {
  local archive="$1"
  local sidecar="$2"
  local expected actual
  expected="$(awk '{print tolower($1)}' "$sidecar")"
  if [[ "${#expected}" -ne 64 ]]; then
    echo "[limrun-litert-lm] Malformed SHA-256 sidecar: $sidecar" >&2
    return 1
  fi
  actual="$(sha256_hex "$archive")"
  if [[ "$actual" != "$expected" ]]; then
    echo "[limrun-litert-lm] SHA-256 mismatch on $archive: got $actual, expected $expected" >&2
    return 1
  fi
  return 0
}

if [[ "$FORCE_REBUILD" -eq 0 ]]; then
  if command -v lim >/dev/null 2>&1 && { [[ -n "${LIM_API_KEY:-}" ]] || [[ -s "$HOME/.lim/config.yaml" ]] || [[ -s "$HOME/.gemini/lim/config.yaml" ]] || [[ -f "$REPO_DIR/.limrun.env" ]]; }; then
    ensure_lim_auth || true
    echo "[limrun-litert-lm] Checking Limrun Asset Storage for $ASSET_NAME..."
    ASSET_ID="$(lim asset list --name "$ASSET_NAME" --json 2>/dev/null | node -e '
      const items = JSON.parse(require("fs").readFileSync(0, "utf8") || "[]");
      process.stdout.write(items[0]?.id || "");
    ' || true)"
    SHA_ID="$(lim asset list --name "$SHA_ASSET_NAME" --json 2>/dev/null | node -e '
      const items = JSON.parse(require("fs").readFileSync(0, "utf8") || "[]");
      process.stdout.write(items[0]?.id || "");
    ' || true)"
    if [[ -n "$ASSET_ID" ]] && lim asset pull "$ASSET_ID" -o "$TMP_WORK" >/dev/null; then
      SIDECAR_OK=1
      if [[ -n "$SHA_ID" ]] && lim asset pull "$SHA_ID" -o "$TMP_WORK" >/dev/null 2>&1; then
        verify_sidecar_file "$TMP_WORK/$ASSET_NAME" "$TMP_WORK/$SHA_ASSET_NAME" || SIDECAR_OK=0
      fi
      if [[ "$SIDECAR_OK" -eq 1 ]]; then
        mkdir -p "$PREBUILT_DIR"
        tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
        printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
        echo "[limrun-litert-lm] Hydrated $PREBUILT_DIR from Limrun Asset Storage ($ASSET_NAME, $ASSET_ID)"
        exit 0
      fi
    fi
  fi

  if [[ "$UPSTREAM_REF" == "$PINNED_REF" ]]; then
    echo "[limrun-litert-lm] Downloading SHA-256-verified public GitHub Release ($PUBLIC_RELEASE_URL)..."
    if curl -fsSL --retry 3 "${PUBLIC_RELEASE_URL}.sha256" -o "$TMP_WORK/release.sha256" \
       && curl -fsSL --retry 3 "$PUBLIC_RELEASE_URL" -o "$TMP_WORK/$ASSET_NAME" \
       && verify_sidecar_file "$TMP_WORK/$ASSET_NAME" "$TMP_WORK/release.sha256"; then
      mkdir -p "$PREBUILT_DIR"
      tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
      printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
      echo "[limrun-litert-lm] Hydrated $PREBUILT_DIR from public SLSA-attested GitHub Release v${CRATE_VERSION}"
      exit 0
    fi
  fi

  if [[ "$HYDRATE_ONLY" -eq 1 ]]; then
    echo "[limrun-litert-lm] Asset $ASSET_NAME not found in Limrun Asset Storage or GitHub Release (--hydrate-only set)." >&2
    exit 1
  fi
fi

if [[ "$HOST_OS" != "Linux" ]]; then
  echo "limrun-build.sh: Remote source rebuild is Linux x86_64; use GitHub Actions release-native.yml for macOS/Windows." >&2
  exit 1
fi

ensure_lim_auth

mkdir -p "$TMP_WORK/ws"
printf 'rootProject.name = "litert-lm-builder"\n' > "$TMP_WORK/ws/settings.gradle"

if [[ -n "${LIM_SANDBOX_ID:-}" ]]; then
  SANDBOX_ID="$LIM_SANDBOX_ID"
  echo "[limrun-litert-lm] Reusing existing sandbox $SANDBOX_ID"
else
  echo "[limrun-litert-lm] Provisioning 96-vCPU Limrun sandbox for google-ai-edge/LiteRT-LM @ $SHORT_REF..."
  SANDBOX_JSON="$(lim gradle create --display-name "litert-lm-${SHORT_REF}" --inactivity-timeout 30m --hard-timeout 60m --label "project=litert-lm-rust" --json)"
  SANDBOX_ID="$(node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(0, "utf8")).metadata.id)' <<<"$SANDBOX_JSON")"
  echo "[limrun-litert-lm] Using sandbox $SANDBOX_ID"
fi

env -C "$TMP_WORK/ws" lim gradle sync --id "$SANDBOX_ID" >/dev/null

CACHE_TOKEN="$(lim access-token create --permission 'remotecache:*:all' --permission 'remoteexecution:*:all' --ttl-seconds 3600 --quiet || true)"
SETUP_JSON="$(NODE_USE_ENV_PROXY=1 node -e '
  const path = require("path"), fs = require("fs"), cp = require("child_process");
  const limBin = fs.realpathSync(cp.execFileSync("which", ["lim"], { encoding: "utf8" }).trim());
  const limRoot = path.resolve(path.dirname(limBin), "..");
  const Limrun = require(path.join(limRoot, "node_modules/@limrun/api")).default;
  const { readConfig } = require(path.join(limRoot, "dist/lib/config"));
  const { getBazelSetup } = require(path.join(limRoot, "dist/lib/bazel-setup"));
  const cfg = readConfig();
  const client = new Limrun({ apiKey: process.env.LIM_API_KEY || cfg.apiKey, baseURL: cfg.apiEndpoint });
  getBazelSetup(client).then(j => process.stdout.write(JSON.stringify(j))).catch(() => process.stdout.write("{}"));
' || echo '{}')"
RBE_ENDPOINT="$(node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(0, "utf8")).endpoint || "")' <<<"$SETUP_JSON")"
RBE_INSTANCE="$(node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(0, "utf8")).instanceName || "")' <<<"$SETUP_JSON")"

pass=0
while true; do
  pass=$((pass + 1))
  echo "[limrun-litert-lm] Bazel compile pass #${pass}..."
  set +e
  env -C "$TMP_WORK/ws" lim gradle run \
    --id "$SANDBOX_ID" \
    --no-sync \
    --timeout 420 \
    --env "UPSTREAM_REPO=$UPSTREAM_REPO" \
    --env "UPSTREAM_REF=$UPSTREAM_REF" \
    --env "LIM_CACHE_TOKEN=$CACHE_TOKEN" \
    --env "RBE_ENDPOINT=$RBE_ENDPOINT" \
    --env "RBE_INSTANCE=$RBE_INSTANCE" \
    -- bash -ce '
      mkdir -p "$HOME/.local/bin"
      export PATH="$HOME/.local/bin:$PATH"
      if ! command -v bazelisk >/dev/null 2>&1; then
        curl -fsSL https://github.com/bazelbuild/bazelisk/releases/download/v1.25.0/bazelisk-linux-amd64 -o "$HOME/.local/bin/bazelisk"
        chmod +x "$HOME/.local/bin/bazelisk"
        ln -sfn "$HOME/.local/bin/bazelisk" "$HOME/.local/bin/bazel"
      fi
      if [ ! -d /tmp/LiteRT-LM ]; then
        GIT_LFS_SKIP_SMUDGE=1 git clone --no-checkout "$UPSTREAM_REPO" /tmp/LiteRT-LM
      fi
      if [ "$(git -C /tmp/LiteRT-LM rev-parse HEAD 2>/dev/null || true)" != "$UPSTREAM_REF" ]; then
        git -C /tmp/LiteRT-LM fetch --depth 1 origin "$UPSTREAM_REF"
        GIT_LFS_SKIP_SMUDGE=1 git -C /tmp/LiteRT-LM checkout -f FETCH_HEAD
        git -C /tmp/LiteRT-LM lfs pull --include="prebuilt/linux_x86_64/*"
      fi
      if [ ! -f /tmp/static-cxx/libstdc++.so ] || [ ! -f /tmp/litert_glibc_compat.o ]; then
        cat > /tmp/litert_glibc_compat.c <<'\''CEOF'\''
long compat_strtol(const char *, char **, int);
unsigned long compat_strtoul(const char *, char **, int);
long long compat_strtoll(const char *, char **, int);
unsigned long long compat_strtoull(const char *, char **, int);
long long compat_strtoll_l(const char *, char **, int, void *);
unsigned long long compat_strtoull_l(const char *, char **, int, void *);
int compat_vsscanf(const char *, const char *, __builtin_va_list);
double compat_fmod(double, double);
float compat_fmodf(float, float);

__asm__(".symver compat_strtol, strtol@GLIBC_2.2.5");
__asm__(".symver compat_strtoul, strtoul@GLIBC_2.2.5");
__asm__(".symver compat_strtoll, strtoll@GLIBC_2.2.5");
__asm__(".symver compat_strtoull, strtoull@GLIBC_2.2.5");
__asm__(".symver compat_strtoll_l, strtoll_l@GLIBC_2.3.3");
__asm__(".symver compat_strtoull_l, strtoull_l@GLIBC_2.3.3");
__asm__(".symver compat_vsscanf, vsscanf@GLIBC_2.2.5");
__asm__(".symver compat_fmod, fmod@GLIBC_2.2.5");
__asm__(".symver compat_fmodf, fmodf@GLIBC_2.2.5");

__attribute__((visibility("hidden"))) long __isoc23_strtol(const char *n, char **e, int b) { return compat_strtol(n, e, b); }
__attribute__((visibility("hidden"))) unsigned long __isoc23_strtoul(const char *n, char **e, int b) { return compat_strtoul(n, e, b); }
__attribute__((visibility("hidden"))) long long __isoc23_strtoll(const char *n, char **e, int b) { return compat_strtoll(n, e, b); }
__attribute__((visibility("hidden"))) unsigned long long __isoc23_strtoull(const char *n, char **e, int b) { return compat_strtoull(n, e, b); }
__attribute__((visibility("hidden"))) long long __isoc23_strtoll_l(const char *n, char **e, int b, void *l) { return compat_strtoll_l(n, e, b, l); }
__attribute__((visibility("hidden"))) unsigned long long __isoc23_strtoull_l(const char *n, char **e, int b, void *l) { return compat_strtoull_l(n, e, b, l); }
__attribute__((visibility("hidden"))) int __isoc23_vsscanf(const char *s, const char *f, __builtin_va_list ap) { return compat_vsscanf(s, f, ap); }
__attribute__((visibility("hidden"))) int __isoc23_sscanf(const char *s, const char *f, ...) {
  __builtin_va_list ap; __builtin_va_start(ap, f); int r = compat_vsscanf(s, f, ap); __builtin_va_end(ap); return r;
}
__attribute__((visibility("hidden"))) double fmod(double x, double y) { return compat_fmod(x, y); }
__attribute__((visibility("hidden"))) float fmodf(float x, float y) { return compat_fmodf(x, y); }
CEOF
        clang -O2 -fPIC -fno-builtin -c /tmp/litert_glibc_compat.c -o /tmp/litert_glibc_compat.o
        rm -rf /tmp/static-cxx
        mkdir -p /tmp/static-cxx
        cp -f "$(clang++ -print-file-name=libstdc++.a)" /tmp/static-cxx/libstdc++.so
        ar rcs /tmp/static-cxx/libstdc++.so /tmp/litert_glibc_compat.o
      fi
      CACHE_FLAGS=()
      if [ -n "${RBE_ENDPOINT:-}" ] && [ -n "${LIM_CACHE_TOKEN:-}" ]; then
        CACHE_FLAGS=(
          --remote_cache="$RBE_ENDPOINT"
          --remote_instance_name="$RBE_INSTANCE"
          --remote_header="Authorization=Bearer $LIM_CACHE_TOKEN"
          --remote_upload_local_results=true
          --remote_cache_compression
          --remote_local_fallback=true
        )
      fi
      cd /tmp/LiteRT-LM
      timeout --signal=INT --kill-after=15 300 bazel \
        --output_user_root=/tmp/bazel-litert \
        build "${CACHE_FLAGS[@]}" \
        --repo_env=GIT_LFS_SKIP_SMUDGE=1 \
        --config=linux_x86_64 \
        --cxxopt=-std=c++20 \
        --host_cxxopt=-std=c++20 \
        --java_runtime_version=remotejdk_17 \
        --linkopt=/tmp/litert_glibc_compat.o \
        --linkopt=-L/tmp/static-cxx \
        --linkopt=-Wl,--exclude-libs,libstdc++.so:libstdc++.a \
        --linkopt=-Wl,--defsym=pidfd_spawnp=0 \
        --linkopt=-Wl,--defsym=pidfd_getpid=0 \
        --linkopt='\''-Wl,-rpath,$ORIGIN'\'' \
        --jobs=96 \
        //c:litert-lm
    '
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    break
  fi
  if [[ "$rc" -ne 124 && "$rc" -ne 130 || "$pass" -ge 12 ]]; then
    echo "[limrun-litert-lm] Bazel failed with exit code $rc on pass #$pass" >&2
    exit "$rc"
  fi
done

env -C "$TMP_WORK/ws" lim gradle run \
  --id "$SANDBOX_ID" \
  --no-sync \
  --timeout 300 \
  --env "UPSTREAM_REF=$UPSTREAM_REF" \
  -- bash -ce '
    WORKSPACE_OUT="$PWD"
    cd /tmp/LiteRT-LM
    rm -rf /tmp/litert-pkg
    mkdir -p /tmp/litert-pkg
    cp -L bazel-bin/c/liblitert-lm.so /tmp/litert-pkg/liblitert-lm.so
    if [ -d prebuilt/linux_x86_64 ]; then
      cp -L prebuilt/linux_x86_64/*.so /tmp/litert-pkg/ 2>/dev/null || true
    fi
    (command -v patchelf >/dev/null 2>&1 || (sudo apt-get update && sudo apt-get install -y patchelf))
    for so in /tmp/litert-pkg/*.so; do
      [ -f "$so" ] && chmod 0755 "$so" && patchelf --set-rpath "\$ORIGIN" "$so"
    done
    ln -sf liblitert-lm.so /tmp/litert-pkg/litert-lm.so
    cp -L LICENSE /tmp/litert-pkg/LICENSE 2>/dev/null || true
    printf "{\"upstream_repo\":\"google-ai-edge/LiteRT-LM\",\"upstream_sha\":\"%s\",\"platform\":\"linux-x86_64\"}\n" "$UPSTREAM_REF" > /tmp/litert-pkg/BUILD_INFO.json
    tar -czf "$WORKSPACE_OUT/litert-lm-out.tar.gz" -C /tmp/litert-pkg .
  '

env -C "$TMP_WORK/ws" lim gradle pull-file litert-lm-out.tar.gz "$TMP_WORK/$ASSET_NAME" --id "$SANDBOX_ID"
printf '%s  %s\n' "$(sha256_hex "$TMP_WORK/$ASSET_NAME")" "$ASSET_NAME" > "$TMP_WORK/$SHA_ASSET_NAME"

for target_name in "$ASSET_NAME" "$SHA_ASSET_NAME"; do
  for old_id in $(lim asset list --name "$target_name" --json 2>/dev/null | node -e '
    for (const item of JSON.parse(require("fs").readFileSync(0, "utf8") || "[]")) console.log(item.id);
  ' || true); do
    lim asset delete "$old_id" >/dev/null 2>&1 || true
  done
done
lim asset push "$TMP_WORK/$ASSET_NAME" -n "$ASSET_NAME" >/dev/null
lim asset push "$TMP_WORK/$SHA_ASSET_NAME" -n "$SHA_ASSET_NAME" >/dev/null

mkdir -p "$PREBUILT_DIR"
tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
echo "[limrun-litert-lm] Built on Limrun, pushed $ASSET_NAME (+ .sha256), and hydrated $PREBUILT_DIR"
