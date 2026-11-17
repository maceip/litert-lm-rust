# litert-lm-rust

Safe, idiomatic Rust bindings for [LiteRT-LM](https://github.com/google-ai-edge/LiteRT-LM) — Google's on-device LLM runtime.

This crate wraps the stable **C API** (`c/engine.h` + `c/conversation.h`), which is the supported FFI surface for the C++ Conversation / Engine stack described in [`litert.md`](./litert.md). The headers are synced with upstream LiteRT-LM C API **v1.1.0** (status-code ABI).

## Features

| Feature | Support |
| --- | --- |
| Conversation API (`send_message` / streaming) | ✅ |
| Multimodal (vision + audio backends, image/audio parts) | ✅ |
| **MTP** (Multi-Token Prediction / speculative decoding) | ✅ |
| Session API (prefill / decode / generate_content) | ✅ |
| Tools, thinking, constrained decoding, LoRA, benchmarks | ✅ |

## Install

```toml
[dependencies]
litert-lm-rust = { git = "https://github.com/maceip/litert-lm-rust", tag = "v0.3.1" }
```

On **Linux `x86_64`** (`x86_64-unknown-linux-gnu`), **macOS `arm64`** (`aarch64-apple-darwin`), and **Windows `x86_64`** (`x86_64-pc-windows-msvc`), `build.rs` (`download-native` enabled by default) automatically downloads the SLSA-attested LiteRT-LM shared libraries from [GitHub Releases](https://github.com/maceip/litert-lm-rust/releases) and verifies each file's SHA-256 checksum before linking.

## Quick start

```rust
use litert_lm_rust::{Backend, Engine, Message};

fn main() -> litert_lm_rust::Result<()> {
    let engine = Engine::builder("model.litertlm")
        .backend(Backend::Cpu)
        .build()?;
    let mut conversation = engine.create_conversation(Default::default())?;
    let reply = conversation.send_message(Message::user("Hello!"))?;
    println!("{reply}");
    Ok(())
}
```

### Multimodal

```rust
use litert_lm_rust::{Backend, ContentPart, Engine, Message};

let engine = Engine::builder("gemma.litertlm")
    .backend(Backend::Cpu)
    .vision_backend(Backend::Gpu)
    .audio_backend(Backend::Cpu)
    .build()?;

let mut conversation = engine.create_conversation(Default::default())?;
let reply = conversation.send_message(Message::user_parts([
    ContentPart::text("Describe this image:"),
    ContentPart::image_path("photo.jpg"),
])?)?;
```

### MTP (Multi-Token Prediction)

```rust
let engine = Engine::builder("model.litertlm")
    .backend(Backend::Gpu)
    .multi_token_prediction(true) // enable_speculative_decoding
    .build()?;
```

## Native library

### Automatic Verified Download (`download-native`, Default)

The `download-native` feature (enabled by default) downloads prebuilt native libraries from `https://github.com/maceip/litert-lm-rust/releases/download/v<version>` (overridable via `LITERT_LM_RELEASE_URL`) into `<OUT_DIR>/prebuilt/` when no local library is found, verifying every file against its `.sha256` sidecar before linking.

Releases are built from pinned upstream `google-ai-edge/LiteRT-LM` (`c/UPSTREAM_COMMIT`) via `.github/workflows/release-native.yml` (backed by Limrun 96-vCPU sandboxes, Limrun RBE remote cache, and Limrun Asset Storage) and signed with GitHub Sigstore OIDC **SLSA v1.0 build provenance** and attested **SPDX 2.3 SBOMs**.

**Published platform bundles (`v0.3.1`):**
- **Linux (`x86_64-unknown-linux-gnu`)**: `litert-lm-v0.3.1-x86_64-unknown-linux-gnu.tar.gz` (`liblitert-lm.so`, `libGemmaModelConstraintProvider.so`, `libwebgpu_dawn.so`, `glibc >= 2.36`, `$ORIGIN` `RUNPATH`)
- **macOS (`aarch64-apple-darwin`)**: `litert-lm-v0.3.1-aarch64-apple-darwin.tar.gz` (`liblitert-lm.dylib`, `libGemmaModelConstraintProvider.dylib`, `libwebgpu_dawn.dylib`, `@loader_path` `LC_RPATH`)
- **Windows (`x86_64-pc-windows-msvc`)**: `litert-lm-v0.3.1-x86_64-pc-windows-msvc.tar.gz` (`litert-lm.dll`, `litert-lm.lib`, `litert-lm.if.lib`, `libGemmaModelConstraintProvider.dll`, `webgpu_dawn.dll`)
- `SHA256SUMS.txt` + per-file `.sha256` sidecars + per-platform `.spdx.json` SBOMs

**Verifying release authenticity manually:**

```bash
for target in x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-pc-windows-msvc; do
  archive="litert-lm-v0.3.1-${target}.tar.gz"
  gh release download v0.3.1 --repo maceip/litert-lm-rust -p "$archive" -p "SHA256SUMS.txt" --clobber
  gh attestation verify "$archive" --repo maceip/litert-lm-rust
  gh attestation verify "$archive" --repo maceip/litert-lm-rust --predicate-type https://spdx.dev/Document/v2.3
done
```

### Using in another project

`build.rs` searches for native libraries in the following order (skipping network download whenever a local copy is present):

1. `LITERT_LM_LIB_DIR` environment variable
2. `<OUT_DIR>/prebuilt/` (auto-downloaded location when `download-native` is enabled)
3. `<crate_dir>/prebuilt/` (`prebuilt/{macos,windows,linux}/`)
4. `<crate_dir>/native/` (`native/{macos,windows,linux}/`)
5. `<crate_dir>/c/` and `<crate_dir>/c/build/`

On Linux and macOS, `build.rs` embeds `-Wl,-rpath,<dir>`, so executables resolve companion libraries without setting `LD_LIBRARY_PATH` or `DYLD_LIBRARY_PATH`. On Windows, `build.rs` automatically stages companion `.dll` files into Cargo's active profile directory (`target/<profile>/`, `deps/`, and `examples/`).

### Building or Hydrating via Limrun (`scripts/limrun-build.sh`)

Contributors and cloud agents can hydrate `prebuilt/` from Limrun Asset Storage (falling back to the SHA-256-verified public GitHub Release) or compile `google-ai-edge/LiteRT-LM` (`//c:litert-lm` at `c/UPSTREAM_COMMIT`) on an ephemeral 96-vCPU Limrun sandbox:

```bash
./scripts/limrun-build.sh                  # hydrate prebuilt/ or build on Limrun
./scripts/limrun-build.sh --force-rebuild  # rebuild //c:litert-lm on 96-vCPU Limrun sandbox
./scripts/fetch-clef-model.sh              # fetch/link SHA-256-verified gemma-4-E2B-it.litertlm
cargo test --test basic -- --ignored --nocapture
```

### Manual Setup

If you prefer to build the LiteRT-LM C API from source locally:

```bash
git clone https://github.com/google-ai-edge/LiteRT-LM
cd LiteRT-LM
git checkout "$(cat /path/to/litert-lm-rust/c/UPSTREAM_COMMIT)"
bazel build //c:litert-lm --config=linux
```

Then point this crate at the output:

```bash
# Linux
export LITERT_LM_LIB_DIR=$PWD/bazel-bin/c

# Windows (PowerShell)
$env:LITERT_LM_LIB_DIR = "C:\path\to\bazel-bin\c"
```

Alternatively, place the shared library in this crate's `c/`, `native/`, or `prebuilt/` folder.

| Variable | Meaning |
| --- | --- |
| `LITERT_LM_LIB_DIR` | Directory containing the shared library |
| `LITERT_LM_LIB_NAME` | Library name without `lib` / extension (default tries `litert-lm` / `LiteRtLmC` / `engine`) |
| `LITERT_LM_STATIC` | If set, link statically |
| `LITERT_LM_INCLUDE_DIR` | Override header path (defaults to vendored `c/`) |

**Note:** To disable automatic download and use your own libraries, add `default-features = false` to your Cargo.toml:
```toml
[dependencies]
litert-lm-rust = { version = "0.3", default-features = false }
```

### Legacy pre‑built Windows binaries

If you have the pre‑built LiteRT‑LM binaries (from the `LiteRT-LM/prebuilt/windows_x86_64` folder of the `litert‑lm‑rust` repository) you can avoid building from source.

1. **Place the folder** somewhere accessible, e.g. as a sibling of your project:
   ```
   <workspace_root>/litert-lm-rust/LiteRT-LM/prebuilt/windows_x86_64
   ```
   This directory should contain `LiteRt.dll` (or `libLiteRt.dll`) and the generated import library `LiteRt.lib` (or `libLiteRt.lib`). If you only have the DLL, generate the import library with:
   ```sh
   llvm-objdump -p libLiteRt.dll > symbols.txt
   llvm-dlltool --def=symbols.txt --output-lib=LiteRt.lib
   ```
2. **Linking**: The crate’s `build.rs` automatically locates this directory on Windows (it walks up from `CARGO_MANIFEST_DIR` to find the sibling `litert‑lm‑rust` folder). No additional environment variables are required unless you want to override the location. To override, set:
   ```powershell
   $env:LITERT_LM_LIB_DIR = "C:\\path\\to\\windows_x86_64"
   $env:LITERT_LM_LIB_NAME = "LiteRt"   # or "libLiteRt"
   ```
3. **What the build script does**:
   - Adds the directory to the linker search path.
   - Links `LiteRt` as a dynamic library.
   - Copies the runtime DLL(s) next to the final executable so they are found at runtime.

After these steps, `cargo build` (or `cargo tauri build`) should succeed without the `LINK : fatal error LNK1181: cannot open input file 'LiteRt.lib'` error.

The `c/` directory also vendors the LiteRT C/C++ SDK headers used when building LiteRT-LM itself.

## Examples

```bash
cargo run --example text_chat -- model.litertlm "Hello"
cargo run --example multimodal -- model.litertlm photo.jpg
cargo run --example mtp -- model.litertlm
```

## Regenerating bindings

```bash
cargo build --features bindgen,docs-only
```

This updates `src/bindings.rs` from `c/wrapper.h` / `c/engine.h` / `c/conversation.h`.

Bindings also stay in sync automatically via GitHub Actions (`update-bindings.yml`): on every push and on a 3-day schedule the C API headers are re-synced from upstream LiteRT-LM and `src/bindings.rs` is regenerated, committing only when something changed. Automatic runs are rate-limited to at most one update per 3 days; `workflow_dispatch` runs the full sync immediately (accelerated).

## Publish

```bash
cargo publish --dry-run
# requires network + crates.io token for a real publish:
# cargo publish
```

Docs builds use `--features docs-only` so they do not need a local native library.

## License

Apache-2.0. LiteRT-LM headers and runtime are © The ODML Authors / Google LLC under Apache-2.0.
