#!/usr/bin/env bash
# Copyright 2026 LiteRT-LM Rust Contributors
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

# Build or hydrate upstream `google-ai-edge/LiteRT-LM` (`//c:litert-lm`) using
# Limrun 96-vCPU Linux sandboxes, Limrun Asset Storage, or the SLSA-attested
# public GitHub Release fallback.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="${LITERT_LM_RUST_DIR:-$(cd "$SCRIPT_DIR/.." && pwd)}"
UPSTREAM_REPO="https://github.com/google-ai-edge/LiteRT-LM.git"
DEFAULT_UPSTREAM_REF="083c49c4a088ba7675186292f43c92d8f5464056"
PINNED_REF="083c49c4a088ba7675186292f43c92d8f5464056"
PINNED_SHA256="4c682512b50b02ceff1bba618f196ae67abe83623f9761ee68ad274eeb90e6ed"
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

Hydrates `prebuilt/liblitert-lm.so` (and companion runtime libraries) from Limrun
Asset Storage or the SHA-256-verified public GitHub Release, or builds
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

CRATE_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO_DIR/Cargo.toml" | head -n1)"
CRATE_VERSION="${CRATE_VERSION:-0.3.0}"
PUBLIC_RELEASE_URL="https://github.com/maceip/litert-lm-rust/releases/download/v${CRATE_VERSION}/litert-lm-v${CRATE_VERSION}-x86_64-unknown-linux-gnu.tar.gz"

SHORT_REF="${UPSTREAM_REF:0:12}"
ASSET_NAME="litert-lm-linux-x86_64-${SHORT_REF}.tar.gz"
PREBUILT_DIR="$REPO_DIR/prebuilt"
STAMP_FILE="$PREBUILT_DIR/.limrun-stamp"

verify_litert_archive() {
  local archive="$1"
  if [[ "$UPSTREAM_REF" == "$PINNED_REF" ]]; then
    local actual
    actual="$(sha256sum "$archive" | awk '{print $1}')"
    if [[ "$actual" != "$PINNED_SHA256" ]]; then
      echo "[limrun-litert-lm] SHA-256 mismatch on $archive: got $actual, expected $PINNED_SHA256" >&2
      return 1
    fi
  fi
  return 0
}

if [[ "$FORCE_REBUILD" -eq 0 && -f "$PREBUILT_DIR/liblitert-lm.so" && -f "$STAMP_FILE" ]]; then
  if [[ "$(cat "$STAMP_FILE")" == "$ASSET_NAME" ]] \
     && readelf -d "$PREBUILT_DIR/liblitert-lm.so" 2>/dev/null | grep -E 'RPATH|RUNPATH' | grep -q '\$ORIGIN'; then
    echo "[limrun-litert-lm] Already hydrated ($ASSET_NAME) in $PREBUILT_DIR"
    exit 0
  fi
fi

TMP_WORK="$(mktemp -d /tmp/limrun-litert.XXXXXX)"
cleanup() {
  rm -rf "$TMP_WORK"
  if [[ "$KEEP_SANDBOX" -eq 0 && -n "${SANDBOX_ID:-}" ]]; then
    lim gradle delete "$SANDBOX_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ "$FORCE_REBUILD" -eq 0 ]]; then
  if command -v lim >/dev/null 2>&1 && { [[ -n "${LIM_API_KEY:-}" ]] || [[ -s "$HOME/.lim/config.yaml" ]] || [[ -s "$HOME/.gemini/lim/config.yaml" ]] || [[ -f "$REPO_DIR/.limrun.env" ]]; }; then
    ensure_lim_auth || true
    echo "[limrun-litert-lm] Checking Limrun Asset Storage for $ASSET_NAME..."
    ASSET_ID="$(lim asset list --name "$ASSET_NAME" --json 2>/dev/null | node -e '
      const items = JSON.parse(require("fs").readFileSync(0, "utf8") || "[]");
      process.stdout.write(items[0]?.id || "");
    ' || true)"
    if [[ -n "$ASSET_ID" ]] && lim asset pull "$ASSET_ID" -o "$TMP_WORK" >/dev/null \
       && verify_litert_archive "$TMP_WORK/$ASSET_NAME"; then
      mkdir -p "$PREBUILT_DIR"
      tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
      printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
      echo "[limrun-litert-lm] Hydrated $PREBUILT_DIR from Limrun Asset Storage ($ASSET_NAME, $ASSET_ID)"
      exit 0
    fi
  fi

  if [[ "$UPSTREAM_REF" == "$PINNED_REF" ]]; then
    echo "[limrun-litert-lm] Downloading SHA-256-pinned public GitHub Release ($PUBLIC_RELEASE_URL)..."
    if curl -fsSL --retry 3 "$PUBLIC_RELEASE_URL" -o "$TMP_WORK/$ASSET_NAME" \
       && verify_litert_archive "$TMP_WORK/$ASSET_NAME"; then
      mkdir -p "$PREBUILT_DIR"
      tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
      printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
      echo "[limrun-litert-lm] Hydrated $PREBUILT_DIR from public SLSA-attested GitHub Release v${CRATE_VERSION}"
      exit 0
    fi
  fi

  if [[ "$HYDRATE_ONLY" -eq 1 ]]; then
    echo "[limrun-litert-lm] Asset $ASSET_NAME not found in Limrun Asset Storage (--hydrate-only set)." >&2
    exit 1
  fi
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

BUILD_ARGS=(
  build //c:litert-lm
  --config=linux
  --jobs=96
  --build_tag_filters=-android
  --copt=-D_GLIBCXX_USE_CXX11_ABI=1
  --linkopt=-L/tmp/static-cxx
  --linkopt=-Wl,--exclude-libs,libstdc++.a
  --linkopt=-Wl,-rpath,\$ORIGIN
)

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
      cd /tmp/LiteRT-LM
      timeout --signal=INT --kill-after=15 300 bazel \
        --output_user_root=/tmp/bazel-litert \
        build //c:litert-lm \
        --config=linux \
        --jobs=96 \
        --build_tag_filters=-android \
        --copt=-D_GLIBCXX_USE_CXX11_ABI=1 \
        --linkopt=-L/tmp/static-cxx \
        --linkopt=-Wl,--exclude-libs,libstdc++.a \
        --linkopt='\''-Wl,-rpath,$ORIGIN'\''
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
    ln -sf liblitert-lm.so /tmp/litert-pkg/litert-lm.so
    if [ -d prebuilt/linux_x86_64 ]; then
      cp -L prebuilt/linux_x86_64/*.so /tmp/litert-pkg/ 2>/dev/null || true
    fi
    cp -L LICENSE /tmp/litert-pkg/LICENSE 2>/dev/null || true
    chmod 0755 /tmp/litert-pkg/*.so 2>/dev/null || true
    printf "{\"upstream_repo\":\"google-ai-edge/LiteRT-LM\",\"upstream_sha\":\"%s\",\"platform\":\"linux-x86_64\"}\n" "$UPSTREAM_REF" > /tmp/litert-pkg/BUILD_INFO.json
    tar -czf "$WORKSPACE_OUT/litert-lm-out.tar.gz" -C /tmp/litert-pkg .
  '

env -C "$TMP_WORK/ws" lim gradle pull-file litert-lm-out.tar.gz "$TMP_WORK/$ASSET_NAME" --id "$SANDBOX_ID"
for old_id in $(lim asset list --name "$ASSET_NAME" --json 2>/dev/null | node -e '
  for (const item of JSON.parse(require("fs").readFileSync(0, "utf8") || "[]")) console.log(item.id);
' || true); do
  lim asset delete "$old_id" >/dev/null 2>&1 || true
done
lim asset push "$TMP_WORK/$ASSET_NAME" -n "$ASSET_NAME" >/dev/null

mkdir -p "$PREBUILT_DIR"
tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
echo "[limrun-litert-lm] Built on Limrun, pushed $ASSET_NAME, and hydrated $PREBUILT_DIR"
