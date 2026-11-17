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
litert-lm-rust = { git = "https://github.com/maceip/litert-lm-rust", tag = "v0.3.0" }
```

On `x86_64-unknown-linux-gnu`, `build.rs` (`download-native` enabled by default) automatically downloads the portable LiteRT-LM shared libraries from [GitHub Releases](https://github.com/maceip/litert-lm-rust/releases) (`glibc >= 2.36`, static `libstdc++`, `$ORIGIN` `RUNPATH`) and verifies each file's SHA-256 checksum before linking.

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

Native Linux releases are built from pinned upstream `google-ai-edge/LiteRT-LM` (`c/UPSTREAM_COMMIT`) on a 96-vCPU Limrun sandbox via `.github/workflows/release-native.yml` and signed with GitHub Sigstore OIDC **SLSA v1.0 build provenance** and an attested **SPDX 2.3 SBOM**.

**Linux (`x86_64-unknown-linux-gnu`) release assets:**
- `litert-lm-v0.3.0-x86_64-unknown-linux-gnu.tar.gz` (+ `.sha256` and `.spdx.json`) — complete portable archive
- `liblitert-lm.so` (+ `.sha256`) — main LiteRT-LM C API v1.1.0 shared library
- `libGemmaModelConstraintProvider.so` (+ `.sha256`) — Gemma structured/constrained decoding provider
- `libwebgpu_dawn.so` (+ `.sha256`) — Dawn WebGPU runtime
- `SHA256SUMS.txt` — combined SHA-256 manifest

**Verifying release authenticity manually:**

```bash
curl -fsSLO https://github.com/maceip/litert-lm-rust/releases/download/v0.3.0/litert-lm-v0.3.0-x86_64-unknown-linux-gnu.tar.gz
curl -fsSLO https://github.com/maceip/litert-lm-rust/releases/download/v0.3.0/litert-lm-v0.3.0-x86_64-unknown-linux-gnu.tar.gz.sha256

sha256sum -c litert-lm-v0.3.0-x86_64-unknown-linux-gnu.tar.gz.sha256
gh attestation verify litert-lm-v0.3.0-x86_64-unknown-linux-gnu.tar.gz --repo maceip/litert-lm-rust
gh attestation verify litert-lm-v0.3.0-x86_64-unknown-linux-gnu.tar.gz --repo maceip/litert-lm-rust --predicate-type https://spdx.dev/Document/v2.3
```

### Using in another project

`build.rs` searches for native libraries in the following order (skipping network download whenever a local copy is present):

1. `LITERT_LM_LIB_DIR` environment variable
2. `<OUT_DIR>/prebuilt/` (auto-downloaded location when `download-native` is enabled)
3. `<crate_dir>/prebuilt/`
4. `<crate_dir>/prebuilt/macos/`
5. `<crate_dir>/native/`
6. `<crate_dir>/c/` and `<crate_dir>/c/build/`

On Linux and macOS, `build.rs` embeds `-Wl,-rpath,<dir>` and `liblitert-lm.so` carries `RUNPATH=$ORIGIN`, so executables resolve companion libraries without setting `LD_LIBRARY_PATH`. On Windows, runtime DLLs must be on `PATH` or next to the executable.

### Manual Setup

If you prefer to build the LiteRT-LM C API from source:

```bash
git clone https://github.com/google-ai-edge/LiteRT-LM
cd LiteRT-LM
git checkout <release-tag>   # e.g. v0.14.0
bazel build //c:engine
```

Then point this crate at the output:

```bash
# Linux
export LITERT_LM_LIB_DIR=$PWD/bazel-bin/c
export LITERT_LM_LIB_NAME=engine

# Windows (PowerShell)
$env:LITERT_LM_LIB_DIR = "C:\path\to\bazel-bin\c"
$env:LITERT_LM_LIB_NAME = "engine"
```

Alternatively, place the shared library in this crate's `c/`, `native/`, or `prebuilt/` folder.

| Variable | Meaning |
| --- | --- |
| `LITERT_LM_LIB_DIR` | Directory containing the shared library |
| `LITERT_LM_LIB_NAME` | Library name without `lib` / extension (default tries `LiteRtLmC` / `engine`) |
| `LITERT_LM_STATIC` | If set, link statically |
| `LITERT_LM_INCLUDE_DIR` | Override header path (defaults to vendored `c/`) |

**Note:** To disable automatic download and use your own libraries, add `default-features = false` to your Cargo.toml:
```toml
[dependencies]
litert-lm-rust = { version = "0.13", default-features = false }
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
