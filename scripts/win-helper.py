#!/usr/bin/env python3
# Copyright 2026 LiteRT-LM Rust Contributors
# SPDX-License-Identifier: Apache-2.0
"""Windows release build helpers (longPathAware manifest, PE DLL staging, MSVC .lib, symbol check)."""

import pathlib
import re
import shutil
import subprocess
import sys

LONGPATH_MANIFEST = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
</assembly>
"""


def write_manifest(dest: str) -> None:
    pathlib.Path(dest).write_text(LONGPATH_MANIFEST, encoding="utf-8", newline="\n")


def stage_built_dll(bazel_c_dir: str, out_dll: str) -> None:
    c_dir = pathlib.Path(bazel_c_dir)
    candidates = [
        c_dir / "litert-lm.dll",
        c_dir / "liblitert-lm.dll",
        c_dir / "litert-lm.so",
        c_dir / "liblitert-lm.so",
        c_dir / "litert-lm",
    ]
    chosen = None
    for cand in candidates:
        if cand.is_file() and cand.stat().st_size > 1024 * 1024:
            with cand.open("rb") as f:
                if f.read(2) == b"MZ":
                    chosen = cand
                    break
    if chosen is None:
        for cand in sorted(c_dir.iterdir()):
            if cand.is_file() and cand.stat().st_size > 1024 * 1024:
                with cand.open("rb") as f:
                    if f.read(2) == b"MZ":
                        chosen = cand
                        break
    if chosen is None:
        listing = [p.name for p in sorted(c_dir.iterdir())] if c_dir.is_dir() else []
        sys.exit(f"Could not locate built PE DLL in {c_dir}; found: {listing}")
    print(f"Staging built PE DLL: {chosen} ({chosen.stat().st_size} bytes) -> {out_dll}")
    shutil.copy2(chosen, out_dll)
    deps = subprocess.check_output(["dumpbin.exe", "/dependents", out_dll], text=True)
    print(deps)


def make_import_lib(dll_path: str, def_path: str, lib_path: str) -> None:
    out = subprocess.check_output(["dumpbin.exe", "/exports", dll_path], text=True)
    syms = sorted(set(re.findall(r"\b(litert_lm_[A-Za-z0-9_]+)\b", out)))
    if not syms:
        sys.exit(f"No litert_lm_* exports found in {dll_path}")
    lines = ["LIBRARY litert-lm.dll", "EXPORTS"] + [f"  {s}" for s in syms]
    pathlib.Path(def_path).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    subprocess.check_call(
        ["lib.exe", "/nologo", f"/def:{def_path}", "/machine:x64", f"/out:{lib_path}"]
    )


def verify_exports(bindings_rs: str, dll_path: str) -> None:
    text = pathlib.Path(bindings_rs).read_text(encoding="utf-8")
    want = sorted(set(re.findall(r"\blitert_lm_[a-z0-9_]+\b", text)))
    out = subprocess.check_output(["dumpbin.exe", "/exports", dll_path], text=True)
    have = set(re.findall(r"\blitert_lm_[a-z0-9_]+\b", out))
    missing = [s for s in want if s not in have]
    print(
        f"Verified {len(want) - len(missing)}/{len(want)} C API functions exported by {dll_path}"
    )
    if missing:
        sys.exit(f"Missing C API functions: {missing}")


def patch_upstream(src_root: str) -> None:
    """Fix upstream c/engine.cc #if inside LITERT_LM_C_ASSIGN_OR_RETURN macro for MSVC /Zc:preprocessor."""
    engine_cc = pathlib.Path(src_root) / "c" / "engine.cc"
    text = engine_cc.read_text(encoding="utf-8")
    old = """  LITERT_LM_C_ASSIGN_OR_RETURN(
      ModelAssets model_assets,
      ModelAssets::Create(
#if defined(_WIN32)
          std::make_shared<litert::lm::ScopedFile>(litert::lm::ScopedFile(
              reinterpret_cast<litert::lm::ScopedFile::PlatformFile>(
                  _get_osfhandle(fd))))));
#else
          std::make_shared<litert::lm::ScopedFile>(
              litert::lm::ScopedFile(fd))));
#endif"""
    new = """#if defined(_WIN32)
  auto scoped_file =
      std::make_shared<litert::lm::ScopedFile>(litert::lm::ScopedFile(
          reinterpret_cast<litert::lm::ScopedFile::PlatformFile>(
              _get_osfhandle(fd))));
#else
  auto scoped_file = std::make_shared<litert::lm::ScopedFile>(
      litert::lm::ScopedFile(fd));
#endif
  LITERT_LM_C_ASSIGN_OR_RETURN(
      ModelAssets model_assets,
      ModelAssets::Create(std::move(scoped_file)));"""
    if old in text:
        engine_cc.write_text(text.replace(old, new, 1), encoding="utf-8", newline="\n")
        print(f"Patched MSVC preprocessor macro directive in {engine_cc}")
    elif new in text:
        print(f"Already patched: {engine_cc}")
    else:
        sys.exit(f"ExpectedLITERT_LM_C_ASSIGN_OR_RETURN snippet not found in {engine_cc}")


def main() -> None:
    cmd = sys.argv[1]
    if cmd == "write-manifest":
        write_manifest(sys.argv[2])
    elif cmd == "patch-upstream":
        patch_upstream(sys.argv[2])
    elif cmd == "stage-built-dll":
        stage_built_dll(sys.argv[2], sys.argv[3])
    elif cmd == "make-import-lib":
        make_import_lib(sys.argv[2], sys.argv[3], sys.argv[4])
    elif cmd == "verify-exports":
        verify_exports(sys.argv[2], sys.argv[3])
    else:
        sys.exit(f"Unknown subcommand: {cmd}")


if __name__ == "__main__":
    main()
