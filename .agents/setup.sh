#!/usr/bin/env bash
set -euo pipefail

# Idempotent environment setup for `maceip/litert-lm-rust`.
# Installs `lim` and Rust toolchains if missing, configures environment variables,
# and hydrates `prebuilt/` (`liblitert-lm.so` + companions) from Limrun when authenticated.

REPO_DIR="$(realpath "$(dirname "${BASH_SOURCE[0]}")/..")"
PREBUILT_DIR="$REPO_DIR/prebuilt"

mkdir -p "$HOME/.local/bin" "$HOME/.cargo/bin"
touch "$HOME/.bashrc"
sed -i '/case \$- in/,/esac/d' "$HOME/.bashrc" 2>/dev/null || true

ensure_bashrc_line() {
  local needle="$1"
  local line="$2"
  grep -qF "$needle" "$HOME/.bashrc" 2>/dev/null || printf '%s\n' "$line" >> "$HOME/.bashrc"
}

ensure_bashrc_line 'export RUSTUP_HOME=' 'export RUSTUP_HOME="$HOME/.rustup"'
ensure_bashrc_line 'export CARGO_HOME=' 'export CARGO_HOME="$HOME/.cargo"'
ensure_bashrc_line '$HOME/.local/bin' 'export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"'

for llvm_lib in /usr/lib/llvm-*/lib /usr/lib64/llvm*/lib; do
  if [[ -f "$llvm_lib/libclang.so" ]] || compgen -G "$llvm_lib/libclang.so.*" >/dev/null; then
    ensure_bashrc_line 'export LIBCLANG_PATH=' "export LIBCLANG_PATH=\"$llvm_lib\""
    export LIBCLANG_PATH="$llvm_lib"
    break
  fi
done

ensure_bashrc_line 'export LITERT_LM_LIB_DIR=' "export LITERT_LM_LIB_DIR=\"$PREBUILT_DIR\""
ensure_bashrc_line "$PREBUILT_DIR" "export LD_LIBRARY_PATH=\"$PREBUILT_DIR\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}\""

export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export PATH="$HOME/.local/bin:$CARGO_HOME/bin:$PATH"
export LITERT_LM_LIB_DIR="$PREBUILT_DIR"
export LD_LIBRARY_PATH="$PREBUILT_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

if ! command -v lim >/dev/null 2>&1; then
  npm install --prefix "$HOME/.local" -g lim --no-audit --no-fund
fi

if [[ -s "$HOME/.gemini/lim/config.yaml" && ! -s "$HOME/.lim/config.yaml" ]]; then
  mkdir -p "$HOME/.lim"
  ln -sfn "$HOME/.gemini/lim/config.yaml" "$HOME/.lim/config.yaml"
fi

if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path
fi

chmod +x "$REPO_DIR/.agents/limrun-build-litert-lm.sh"
ln -sfn "$REPO_DIR/.agents/limrun-build-litert-lm.sh" "$HOME/.local/bin/limrun-build-litert-lm"

bash "$REPO_DIR/.agents/limrun-build-litert-lm.sh" --repo "$REPO_DIR" --if-authenticated
