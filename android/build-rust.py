"""Build the shared JNI library for a physical phone and the x86_64 emulator."""
import os
from pathlib import Path
import shutil
import sys
import subprocess

root = Path(__file__).resolve().parents[1]
ndk, output = map(Path, sys.argv[1:])
host = {"linux": "linux-x86_64", "darwin": "darwin-x86_64"}[sys.platform]
for target, abi in [("aarch64-linux-android", "arm64-v8a"), ("x86_64-linux-android", "x86_64")]:
    linker = ndk / "toolchains/llvm/prebuilt" / host / "bin" / (target + "26-clang")
    env = dict(os.environ)
    env["CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER"] = str(linker)
    target_env = target.replace("-", "_")
    env[f"CC_{target_env}"] = str(linker)
    env[f"AR_{target_env}"] = str(linker.with_name("llvm-ar"))
    target_dir = output.parent / "rust-target"
    subprocess.run([
        "cargo", "rustc", "--locked", "--release", "-j2", "-p", "school-day-android", "--lib",
        "--target-dir", target_dir, "--target", target, "--",
        "-C", "link-arg=-Wl,-z,max-page-size=16384",
        "-C", "link-arg=-Wl,-z,common-page-size=16384",
    ], cwd=root, env=env, check=True)
    directory = output / abi
    directory.mkdir(exist_ok=True, parents=True)
    shutil.copy2(target_dir / target / "release/libschool_day_android.so", directory)
