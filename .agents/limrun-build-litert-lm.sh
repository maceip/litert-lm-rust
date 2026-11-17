#!/usr/bin/env bash
set -euo pipefail

# Build or hydrate Google's LiteRT-LM C shared libraries (`liblitert-lm.so` + companions)
# for `maceip/litert-lm-rust` using Limrun 96-vCPU sandboxes and Limrun Asset Storage.

SCRIPT_PATH="$(realpath "${BASH_SOURCE[0]}")"
DEFAULT_REPO_DIR="$(realpath "$(dirname "$SCRIPT_PATH")/..")"
REPO_DIR="${LITERT_LM_RUST_DIR:-$DEFAULT_REPO_DIR}"
UPSTREAM_REPO="${LITERT_LM_UPSTREAM_REPO:-https://github.com/google-ai-edge/LiteRT-LM.git}"
UPSTREAM_REF="${LITERT_LM_REF:-083c49c4a088ba7675186292f43c92d8f5464056}"
FORCE_REBUILD=0
HYDRATE_ONLY=0
KEEP_SANDBOX=0
IF_AUTHENTICATED=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO_DIR="$2"
      shift 2
      ;;
    --ref)
      UPSTREAM_REF="$2"
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
Usage: limrun-build-litert-lm [--repo <dir>] [--ref <commit>] [--force-rebuild] [--hydrate-only] [--if-authenticated] [--keep-sandbox]

Hydrates `prebuilt/` for `litert-lm-rust` from Limrun Asset Storage, or builds
upstream `google-ai-edge/LiteRT-LM` (`//c:litert-lm`) on a 96-vCPU Limrun sandbox
and caches the resulting archive in Limrun Asset Storage.
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

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
  for env_file in "$HOME/upload/tmp.env" "$HOME/tmp.env"; do
    if [[ -f "$env_file" ]]; then
      local key
      key="$(grep -E '^(export[[:space:]]+)?LIM_API_KEY=' "$env_file" | tail -n1 | cut -d= -f2- | tr -d '"' | tr -d "'" | tr -d '\r' || true)"
      if [[ -n "$key" ]]; then
        mkdir -p "$HOME/.gemini/lim" "$HOME/.lim"
        lim set-api-key "$key" >/dev/null
        cp -f "$HOME/.lim/config.yaml" "$HOME/.gemini/lim/config.yaml"
        chmod 600 "$HOME/.gemini/lim/config.yaml" 2>/dev/null || true
        ln -sfn "$HOME/.gemini/lim/config.yaml" "$HOME/.lim/config.yaml"
        return 0
      fi
    fi
  done
  if [[ "$IF_AUTHENTICATED" -eq 1 ]]; then
    echo "[limrun-litert-lm] Skipping Limrun hydration (no Limrun credentials configured; run 'lim login' or set LIM_API_KEY)."
    exit 0
  fi
  echo "limrun-build-litert-lm: No Limrun API key configured (run 'lim login' or set LIM_API_KEY)." >&2
  return 1
}

PREBUILT_DIR="$REPO_DIR/prebuilt"
SHORT_REF="${UPSTREAM_REF:0:12}"
ASSET_NAME="litert-lm-linux-x86_64-${SHORT_REF}.tar.gz"
STAMP_FILE="$PREBUILT_DIR/.limrun-stamp"

if [[ "$FORCE_REBUILD" -eq 0 && -f "$STAMP_FILE" && -f "$PREBUILT_DIR/liblitert-lm.so" ]]; then
  if [[ "$(cat "$STAMP_FILE")" == "$ASSET_NAME" ]]; then
    echo "[limrun-litert-lm] Already hydrated ($ASSET_NAME) in $PREBUILT_DIR"
    exit 0
  fi
fi

ensure_lim_auth

TMP_WORK="$(mktemp -d /tmp/limrun-litert.XXXXXX)"
cleanup() {
  rm -rf "$TMP_WORK"
  if [[ "$KEEP_SANDBOX" -eq 0 && -n "${SANDBOX_ID:-}" ]]; then
    lim gradle delete "$SANDBOX_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ "$FORCE_REBUILD" -eq 0 ]]; then
  echo "[limrun-litert-lm] Checking Limrun Asset Storage for $ASSET_NAME..."
  ASSET_ID="$(lim asset list --name "$ASSET_NAME" --json 2>/dev/null | node -e '
    const items = JSON.parse(require("fs").readFileSync(0, "utf8") || "[]");
    process.stdout.write(items[0]?.id || "");
  ' || true)"
  if [[ -n "$ASSET_ID" ]] && lim asset pull "$ASSET_ID" -o "$TMP_WORK" >/dev/null; then
    mkdir -p "$PREBUILT_DIR"
    tar -xzf "$TMP_WORK/$ASSET_NAME" -C "$PREBUILT_DIR"
    printf '%s\n' "$ASSET_NAME" > "$STAMP_FILE"
    echo "[limrun-litert-lm] Hydrated $PREBUILT_DIR from Limrun Asset Storage ($ASSET_NAME, $ASSET_ID)"
    exit 0
  fi
  if [[ "$HYDRATE_ONLY" -eq 1 ]]; then
    echo "[limrun-litert-lm] Asset $ASSET_NAME not found in Limrun Asset Storage (--hydrate-only set)." >&2
    exit 1
  fi
fi

mkdir -p "$TMP_WORK/ws"
printf 'rootProject.name = "litert-lm-builder"\n' > "$TMP_WORK/ws/settings.gradle"

if [[ -n "${LIM_SANDBOX_ID:-}" ]]; then
  SANDBOX_ID="$LIM_SANDBOX_ID"
  echo "[limrun-litert-lm] Reusing existing sandbox $SANDBOX_ID"
else
  echo "[limrun-litert-lm] Provisioning 96-vCPU Limrun sandbox for google-ai-edge/LiteRT-LM @ $SHORT_REF..."
  SANDBOX_JSON="$(lim gradle create --display-name "litert-lm-${SHORT_REF}" --label "project=litert-lm-rust" --json)"
  SANDBOX_ID="$(node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(0, "utf8")).metadata.id)' <<<"$SANDBOX_JSON")"
  echo "[limrun-litert-lm] Using sandbox $SANDBOX_ID"
fi

env -C "$TMP_WORK/ws" lim gradle sync --id "$SANDBOX_ID" >/dev/null

env -C "$TMP_WORK/ws" lim gradle run \
  --id "$SANDBOX_ID" \
  --no-sync \
  --timeout 3600 \
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
  '

BUILT_OK=0
for attempt in 1 2 3 4; do
  if env -C "$TMP_WORK/ws" lim gradle run \
    --id "$SANDBOX_ID" \
    --no-sync \
    --timeout 360 \
    -- bash -ce '
      export PATH="$HOME/.local/bin:$PATH"
      timeout 300 env -C /tmp/LiteRT-LM bazelisk --output_user_root=/tmp/bazel-litert build \
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
        --jobs=96 \
        //c:litert-lm
    ' ; then
    BUILT_OK=1
    break
  fi
  echo "[limrun-litert-lm] Continuing incremental Bazel build pass ($attempt/4)..."
done
if [[ "$BUILT_OK" -ne 1 ]]; then
  echo "[limrun-litert-lm] Bazel build of //c:litert-lm did not finish within 4 passes." >&2
  exit 1
fi

env -C "$TMP_WORK/ws" lim gradle run \
  --id "$SANDBOX_ID" \
  --no-sync \
  --env "UPSTREAM_REF=$UPSTREAM_REF" \
  -- bash -ce '
    WORKSPACE_OUT="$PWD"
    rm -rf /tmp/litert-pkg
    mkdir -p /tmp/litert-pkg
    cp -L /tmp/LiteRT-LM/bazel-bin/c/liblitert-lm.so /tmp/litert-pkg/liblitert-lm.so
    ln -sfn liblitert-lm.so /tmp/litert-pkg/litert-lm.so
    for f in /tmp/LiteRT-LM/prebuilt/linux_x86_64/*.so; do
      [ -f "$f" ] && cp -L "$f" /tmp/litert-pkg/
    done
    find -L /tmp/LiteRT-LM/bazel-bin -maxdepth 4 -name "libLiteRt*.so" -exec cp -L {} /tmp/litert-pkg/ \; 2>/dev/null || true
    chmod 0755 /tmp/litert-pkg/*.so 2>/dev/null || true
    printf "{\"upstream_repo\":\"google-ai-edge/LiteRT-LM\",\"upstream_sha\":\"%s\",\"platform\":\"linux-x86_64\"}\n" "$UPSTREAM_REF" > /tmp/litert-pkg/BUILD_INFO.json
    tar -czf "$WORKSPACE_OUT/litert-lm-out.tar.gz" -C /tmp/litert-pkg .
  '

env -C "$TMP_WORK/ws" lim gradle pull-file litert-lm-out.tar.gz "$TMP_WORK/$ASSET_NAME"
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
