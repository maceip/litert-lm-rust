//! Build script for `litert-lm-rust`.
//!
//! # Linking
//!
//! Linking is controlled by environment variables:
//! - `LITERT_LM_LIB_DIR` — directory containing the LiteRT-LM C shared library
//! - `LITERT_LM_LIB_NAME` — library name without `lib` prefix / extension
//!   (default: tries `litert-lm`, `LiteRtLmC`, `engine` in that order)
//! - `LITERT_LM_INCLUDE_DIR` — optional override for the C header directory
//!
//! On Windows the build script also looks for `<name>.if.lib` (Bazel-style import
//! library) in addition to the standard `<name>.lib` form.
//!
//! Enable the `bindgen` feature to regenerate bindings from `c/wrapper.h`.
//! Enable the `download-native` feature to automatically download native libraries
//! from GitHub releases if not found locally.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    println!("cargo:rerun-if-changed=c/engine.h");
    println!("cargo:rerun-if-changed=c/conversation.h");
    println!("cargo:rerun-if-changed=c/api_export.h");
    println!("cargo:rerun-if-changed=c/error_reporter.h");
    println!("cargo:rerun-if-changed=c/wrapper.h");
    println!("cargo:rerun-if-changed=src/bindings.rs");
    println!("cargo:rerun-if-env-changed=LITERT_LM_LIB_DIR");
    println!("cargo:rerun-if-env-changed=LITERT_LM_LIB_NAME");
    println!("cargo:rerun-if-env-changed=LITERT_LM_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=LITERT_LM_STATIC");
    println!("cargo:rerun-if-env-changed=LITERT_LM_RELEASE_URL");
    println!("cargo:rerun-if-env-changed=LITERT_LM_FORCE_DOWNLOAD");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_path = out_dir.join("bindings.rs");

    // ── Bindgen ───────────────────────────────────────────────────────────────
    #[cfg(feature = "bindgen")]
    {
        let include_dir = env::var_os("LITERT_LM_INCLUDE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| manifest_dir.join("c"));
        generate_bindings(&include_dir, &bindings_path);
    }

    #[cfg(not(feature = "bindgen"))]
    {
        let vendored = manifest_dir.join("src/bindings.rs");
        if vendored.exists() {
            std::fs::copy(&vendored, &bindings_path).expect("copy vendored bindings");
        } else {
            panic!(
                "src/bindings.rs missing. Build once with `--features bindgen` to generate it."
            );
        }
    }

    // ── Skip native linking for docs.rs ─────────────────────────────────────
    if env::var_os("CARGO_FEATURE_DOCS_ONLY").is_some() {
        return;
    }

    // ── Attempt verified download if feature enabled and not present locally ─
    #[cfg(feature = "download-native")]
    {
        let force_download = env::var_os("LITERT_LM_FORCE_DOWNLOAD").is_some();
        if force_download || !has_local_library(&manifest_dir) {
            let prebuilt_dir = out_dir.join("prebuilt");
            if force_download || !has_required_files(&prebuilt_dir) {
                println!(
                    "cargo:warning=Downloading SHA-256 verified LiteRT-LM native libraries..."
                );
                if let Err(e) = download_native_libraries(&prebuilt_dir) {
                    panic!(
                        "litert-lm-rust failed to download verified native libraries: {e}\n\
                         Set LITERT_LM_LIB_DIR to a local directory containing liblitert-lm \
                         or override LITERT_LM_RELEASE_URL."
                    );
                }
            }
        }
    }

    link_native_library(&manifest_dir, &out_dir);
}

// ── Bindgen (optional feature) ────────────────────────────────────────────────
#[cfg(feature = "bindgen")]
fn generate_bindings(include_dir: &PathBuf, bindings_path: &PathBuf) {
    let wrapper = include_dir.join("wrapper.h");
    // Upstream headers include either "api_export.h" (Apple) or
    // "c/api_export.h" (other platforms); both resolve when the include dir
    // and its parent are on the search path.
    let bindings = bindgen::Builder::default()
        .header(wrapper.to_string_lossy())
        .clang_arg(format!("-I{}", include_dir.display()))
        .clang_arg(format!("-I{}", include_dir.parent().unwrap().display()))
        .clang_arg("-DLITERT_LM_C_API_EXPORT=")
        .allowlist_function("litert_lm_.*")
        .allowlist_type("LiteRtLm.*")
        .allowlist_var("kLiteRtLm.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate LiteRT-LM bindings");
    bindings
        .write_to_file(bindings_path)
        .expect("Couldn't write bindings");

    // Keep a checked-in copy for docs.rs / consumers without libclang.
    let vendored = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/bindings.rs");
    let _ = std::fs::copy(bindings_path, vendored);
}

// ── Native library linking ────────────────────────────────────────────────────

/// Library name candidates tried in order when `LITERT_LM_LIB_NAME` is not set.
fn default_lib_names() -> Vec<String> {
    // On Windows the main import library ships as `litert-lm.if.lib` (Bazel
    // style). We also try the conventional names so community prebuilts work.
    vec![
        "litert-lm".to_string(),
        "LiteRtLmC".to_string(),
        "engine".to_string(),
    ]
}

fn resolved_lib_names() -> Vec<String> {
    if let Ok(n) = env::var("LITERT_LM_LIB_NAME") {
        vec![n]
    } else {
        default_lib_names()
    }
}

fn local_search_dirs(manifest_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        env::var_os("LITERT_LM_LIB_DIR").map(PathBuf::from),
        Some(manifest_dir.join("prebuilt")),
        Some(manifest_dir.join("prebuilt/macos")),
        Some(manifest_dir.join("native")),
        Some(manifest_dir.join("c")),
        Some(manifest_dir.join("c/build")),
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_dir())
    .collect();
    dirs.dedup();
    dirs
}

#[cfg(feature = "download-native")]
fn has_local_library(manifest_dir: &std::path::Path) -> bool {
    let names = resolved_lib_names();
    local_search_dirs(manifest_dir)
        .iter()
        .any(|dir| names.iter().any(|name| dir_has_library(dir, name)))
}

fn link_native_library(manifest_dir: &PathBuf, out_dir: &PathBuf) {
    let force_download = env::var_os("LITERT_LM_FORCE_DOWNLOAD").is_some();
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if force_download {
        search_dirs.push(out_dir.join("prebuilt"));
    } else {
        if let Some(env_dir) = env::var_os("LITERT_LM_LIB_DIR").map(PathBuf::from) {
            search_dirs.push(env_dir);
        }
        search_dirs.push(out_dir.join("prebuilt"));
        search_dirs.extend(local_search_dirs(manifest_dir));
    }
    search_dirs.retain(|p| p.is_dir());
    search_dirs.dedup();

    let names = resolved_lib_names();

    for dir in &search_dirs {
        for name in &names {
            if dir_has_library(dir, name) {
                println!("cargo:rustc-link-search=native={}", dir.display());
                emit_link_lib(name, dir);
                emit_platform_extra_libs(dir);
                return;
            }
        }
    }

    let tried: Vec<_> = search_dirs.iter().map(|p| p.display().to_string()).collect();
    println!(
        "cargo:warning=litert-lm-rust: native library not found in [{}]. \
         Set LITERT_LM_LIB_DIR to the folder containing the DLL/shared-lib \
         before linking an executable.",
        tried.join(", ")
    );
    println!("cargo:rustc-link-lib=dylib={}", names[0]);
}

/// Emit the correct `cargo:rustc-link-lib` directive for a found library.
fn emit_link_lib(name: &str, dir: &PathBuf) {
    let is_static = env::var_os("LITERT_LM_STATIC").is_some();

    if cfg!(windows) && !is_static {
        let if_lib = dir.join(format!("{name}.if.lib"));
        if if_lib.exists() {
            println!("cargo:rustc-link-lib=dylib={name}");
            println!("cargo:rustc-link-arg={}", if_lib.display());
            return;
        }
    }

    if is_static {
        println!("cargo:rustc-link-lib=static={name}");
    } else {
        println!("cargo:rustc-link-lib=dylib={name}");
    }
}

fn emit_platform_extra_libs(dir: &std::path::Path) {
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=m");
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=pthread");
        // Embed RUNPATH so tests, examples, and binaries find liblitert-lm.so
        // (and its $ORIGIN companions) without requiring LD_LIBRARY_PATH.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path/../lib");
    }
}

/// Return true if the directory contains any recognisable library file for `name`.
fn dir_has_library(dir: &PathBuf, name: &str) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let candidates = [
        format!("{name}.dll"),
        format!("{name}.lib"),
        format!("{name}.if.lib"),
        format!("lib{name}.so"),
        format!("lib{name}.dylib"),
        format!("lib{name}.dll"),
        format!("lib{name}.a"),
        format!("{name}.a"),
    ];
    candidates.iter().any(|n| dir.join(n).exists())
}

// ── Native library download + SHA-256 verification (optional feature) ─────────

#[cfg(feature = "download-native")]
const DEFAULT_RELEASE_REPO: &str = "https://github.com/maceip/litert-lm-rust";

/// Required native files for the current platform.
#[cfg(feature = "download-native")]
fn required_native_files() -> Result<Vec<&'static str>, String> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok(vec![
            "liblitert-lm.so",
            "libGemmaModelConstraintProvider.so",
            "libwebgpu_dawn.so",
        ])
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok(vec![
            "litert-lm.dll",
            "litert-lm.if.lib",
            "libLiteRt.dll",
            "libGemmaModelConstraintProvider.dll",
            "libLiteRtTopKWebGpuSampler.dll",
            "libLiteRtWebGpuAccelerator.dll",
            "libwebgpu_dawn.dll",
        ])
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok(vec![
            "liblitert-lm.dylib",
            "libLiteRt.dylib",
            "libGemmaModelConstraintProvider.dylib",
            "libLiteRtTopKMetalSampler.dylib",
            "libLiteRtMetalAccelerator.dylib",
        ])
    } else {
        Err(format!(
            "No prebuilt native release configured for {}/{}; set LITERT_LM_LIB_DIR.",
            env::consts::OS,
            env::consts::ARCH
        ))
    }
}

#[cfg(feature = "download-native")]
fn has_required_files(prebuilt_dir: &PathBuf) -> bool {
    let Ok(files) = required_native_files() else {
        return false;
    };
    files.iter().all(|file| {
        let bin_path = prebuilt_dir.join(file);
        let stamp_path = prebuilt_dir.join(format!("{file}.sha256"));
        let Ok(meta) = std::fs::metadata(&bin_path) else {
            return false;
        };
        let Ok(stamp) = std::fs::read_to_string(&stamp_path) else {
            return false;
        };
        let mut parts = stamp.split_whitespace();
        let Some(hex) = parts.next() else {
            return false;
        };
        let Some(expected_len) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
            return false;
        };
        hex.len() == 64 && meta.len() == expected_len
    })
}

#[cfg(feature = "download-native")]
fn release_base_url() -> String {
    env::var("LITERT_LM_RELEASE_URL")
        .unwrap_or_else(|_| format!("{DEFAULT_RELEASE_REPO}/releases/download/v{}", env!("CARGO_PKG_VERSION")))
        .trim_end_matches('/')
        .to_string()
}

#[cfg(feature = "download-native")]
fn download_native_libraries(prebuilt_dir: &PathBuf) -> Result<(), String> {
    let files = required_native_files()?;
    std::fs::create_dir_all(prebuilt_dir)
        .map_err(|e| format!("Failed to create {}: {e}", prebuilt_dir.display()))?;

    let base_url = release_base_url();
    for file in files {
        let url = format!("{base_url}/{file}");
        let sha_url = format!("{url}.sha256");
        let dest_path = prebuilt_dir.join(file);
        let part_path = prebuilt_dir.join(format!("{file}.part"));
        let stamp_path = prebuilt_dir.join(format!("{file}.sha256"));

        let expected_sha = fetch_expected_sha256(&sha_url)?;
        download_file(&url, &part_path)?;

        let (actual_sha, size) = sha256_file(&part_path)?;
        if actual_sha != expected_sha {
            let _ = std::fs::remove_file(&part_path);
            return Err(format!(
                "SHA-256 mismatch for {url}: expected {expected_sha}, got {actual_sha}"
            ));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&part_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed to chmod {}: {e}", part_path.display()))?;
        }

        std::fs::rename(&part_path, &dest_path)
            .map_err(|e| format!("Failed to move {} into place: {e}", dest_path.display()))?;
        std::fs::write(&stamp_path, format!("{actual_sha} {size}\n"))
            .map_err(|e| format!("Failed to write {}: {e}", stamp_path.display()))?;
    }

    Ok(())
}

#[cfg(feature = "download-native")]
fn fetch_expected_sha256(sha_url: &str) -> Result<String, String> {
    let tmp = env::temp_dir().join(format!("litert-lm-sha-{}.txt", std::process::id()));
    download_file(sha_url, &tmp)?;
    let content = std::fs::read_to_string(&tmp)
        .map_err(|e| format!("Failed to read SHA-256 sidecar {sha_url}: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    let hex = content
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("Empty SHA-256 sidecar at {sha_url}"))?
        .to_ascii_lowercase();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("Malformed SHA-256 sidecar at {sha_url}: {hex:?}"));
    }
    Ok(hex)
}

#[cfg(feature = "download-native")]
fn download_file(url: &str, dest: &PathBuf) -> Result<(), String> {
    let proxy_configured = env::var_os("HTTPS_PROXY").is_some()
        || env::var_os("https_proxy").is_some()
        || env::var_os("HTTP_PROXY").is_some()
        || env::var_os("http_proxy").is_some();

    if !proxy_configured {
        if let Ok(()) = download_via_ureq(url, dest) {
            return Ok(());
        }
    }

    if let Ok(status) = std::process::Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(dest)
        .arg(url)
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    download_via_ureq(url, dest)
}

#[cfg(feature = "download-native")]
fn download_via_ureq(url: &str, dest: &PathBuf) -> Result<(), String> {
    let agent = ureq::Agent::new();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("HTTP request failed for {url}: {e}"))?;
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("Failed to create {}: {e}", dest.display()))?;
    std::io::copy(&mut reader, &mut file)
        .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
    Ok(())
}

/// Minimal FIPS 180-4 SHA-256 implementation so `build.rs` verifies downloaded
/// release artifacts cryptographically without adding extra build-dependencies.
#[cfg(feature = "download-native")]
fn sha256_file(path: &PathBuf) -> Result<(String, u64), String> {
    use std::io::Read;

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open {} for SHA-256: {e}", path.display()))?;
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut total_bytes: u64 = 0;
    let mut tail = [0u8; 64];
    let mut tail_len = 0usize;
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        total_bytes += n as u64;
        let mut slice = &buf[..n];
        if tail_len > 0 {
            let need = 64 - tail_len;
            if slice.len() >= need {
                tail[tail_len..64].copy_from_slice(&slice[..need]);
                compress(&mut state, &tail);
                tail_len = 0;
                slice = &slice[need..];
            } else {
                tail[tail_len..tail_len + slice.len()].copy_from_slice(slice);
                tail_len += slice.len();
                slice = &[];
            }
        }
        let mut chunks = slice.chunks_exact(64);
        for chunk in &mut chunks {
            let block: &[u8; 64] = chunk.try_into().unwrap();
            compress(&mut state, block);
        }
        let rem = chunks.remainder();
        if !rem.is_empty() {
            tail[..rem.len()].copy_from_slice(rem);
            tail_len = rem.len();
        }
    }

    tail[tail_len] = 0x80;
    tail_len += 1;
    if tail_len > 56 {
        tail[tail_len..64].fill(0);
        compress(&mut state, &tail);
        tail_len = 0;
    }
    tail[tail_len..56].fill(0);
    tail[56..64].copy_from_slice(&(total_bytes * 8).to_be_bytes());
    compress(&mut state, &tail);

    let mut hex = String::with_capacity(64);
    for word in state {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{word:08x}");
    }
    Ok((hex, total_bytes))
}
