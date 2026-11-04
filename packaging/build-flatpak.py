#!/usr/bin/env python3
"""Build a source-based Flatpak and assets for a GitHub release."""

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib


APP_ID = "io.github.sqeezy_53.school-day"
PROJECT = Path(__file__).resolve().parents[1]


def run(command, *, cwd=None, env=None, stdout=None):
    print("+", " ".join(map(str, command)), flush=True)
    return subprocess.run(command, cwd=cwd, env=env, stdout=stdout, check=True)


def copy_licenses(vendor, destination):
    destination.mkdir(parents=True)
    index = []
    for crate in sorted(vendor.iterdir()):
        manifest = crate / "Cargo.toml"
        if not manifest.is_file():
            continue
        package = tomllib.loads(manifest.read_text())["package"]
        output = destination / crate.name
        output.mkdir()
        candidates = set()
        for pattern in ("LICENSE*", "COPYING*", "NOTICE*", "COPYRIGHT*", "README*"):
            candidates.update(path for path in crate.glob(pattern) if path.is_file())
        if package.get("license-file"):
            path = crate / package["license-file"]
            if path.is_file() and path.resolve().is_relative_to(crate.resolve()):
                candidates.add(path)
        for path in sorted(candidates):
            shutil.copy2(path, output / path.name)
        index.append({key: package.get(key, "") for key in ("name", "version", "license", "repository")})
    (destination / "crates.json").write_text(json.dumps(index, indent=2))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-dir", type=Path, default=PROJECT / ".build/flatpak")
    parser.add_argument("--output-dir", type=Path, default=PROJECT / "dist")
    args = parser.parse_args()
    if platform.machine() != "x86_64":
        raise SystemExit("Этот сценарий собирает выпуск x86_64. Запустите его на машине x86_64.")
    work = args.work_dir.resolve()
    output = args.output_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)
    for name in ("flatpak", "cache", "tmp"):
        (work / name).mkdir(exist_ok=True)
    env = os.environ.copy()
    env.update({
        "FLATPAK_USER_DIR": str(work / "flatpak"),
        "XDG_CACHE_HOME": str(work / "cache"),
        "TMPDIR": str(work / "tmp"),
    })
    run(["flatpak", "--user", "remote-add", "--if-not-exists", "flathub",
         "https://dl.flathub.org/repo/flathub.flatpakrepo"], env=env)
    run(["flatpak", "--user", "install", "-y", "--noninteractive", "--no-related", "flathub",
         "org.flatpak.Builder", "org.gnome.Sdk//51", "org.freedesktop.Sdk.Extension.rust-stable//26.08"], env=env)

    stage = Path(tempfile.mkdtemp(prefix="run-", dir=work))
    source = stage / "source"
    source.mkdir()
    for name in ("src", "packaging", "docs", "Cargo.toml", "Cargo.lock", "build.rs", "README.md"):
        path = PROJECT / name
        if path.is_dir():
            shutil.copytree(path, source / name, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        elif path.is_file():
            shutil.copy2(path, source / name)
    for pattern in ("LICENSE*", "COPYING*"):
        for path in PROJECT.glob(pattern):
            if path.is_file():
                shutil.copy2(path, source / path.name)

    with (stage / "vendor-output.toml").open("w") as config:
        run(["cargo", "vendor", "--locked", "--versioned-dirs", str(work / "vendor")],
            cwd=PROJECT, env=env, stdout=config)
    shutil.copytree(work / "vendor", source / "vendor")
    (source / ".cargo").mkdir()
    (source / ".cargo/config.toml").write_text(
        '[source.crates-io]\nreplace-with = "vendored-sources"\n'
        '[source.vendored-sources]\ndirectory = "vendor"\n'
    )
    copy_licenses(source / "vendor", source / "packaging/third-party-licenses")
    version = tomllib.loads((source / "Cargo.toml").read_text())["package"]["version"]
    manifest_path = source / "packaging/flatpak" / f"{APP_ID}.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["modules"][0]["sources"][0]["path"] = "source"
    manifest_path = stage / f"{APP_ID}.json"
    manifest_path.write_text(json.dumps(manifest, indent=2))

    # Запуск на хосте сохраняет отдельный каталог SDK при вложенных командах.
    builder_binary = shutil.which("flatpak-builder")
    if builder_binary is None:
        location = subprocess.check_output(
            ["flatpak", "--user", "info", "--show-location", "org.flatpak.Builder"],
            env=env, text=True,
        ).strip()
        builder_binary = str(Path(location) / "files/bin/flatpak-builder")
        try:
            run([builder_binary, "--version"], env=env)
        except subprocess.CalledProcessError as error:
            raise SystemExit("Установите системный flatpak-builder и повторите сборку.") from error
    builder = [
        builder_binary,
        "--user", "--disable-rofiles-fuse", "--install-deps-from=flathub", f"--state-dir={work / 'builder-state'}",
        f"--repo={stage / 'repo'}", str(stage / "build"), str(manifest_path),
    ]
    run(builder, cwd=stage, env=env)
    bundle = output / f"school-day-{version}-x86_64.flatpak"
    staged_bundle = stage / bundle.name
    run(["flatpak", "build-bundle", str(stage / "repo"), str(staged_bundle), APP_ID,
         "--runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo"], env=env)
    shutil.copy2(staged_bundle, bundle)

    archive = output / f"school-day-{version}-source.tar.gz"
    with tarfile.open(archive, "w:gz") as destination:
        destination.add(source, arcname=f"school-day-{version}")
    provenance = {
        "version": version,
        "app_id": APP_ID,
        "base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=PROJECT, text=True).strip(),
        "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=PROJECT)),
        "source_archive": archive.name,
        "files": {str(path.relative_to(source)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in sorted(source.rglob("*")) if path.is_file() and "vendor" not in path.relative_to(source).parts},
    }
    (output / "SOURCE.json").write_text(json.dumps(provenance, ensure_ascii=False, indent=2))
    shutil.copy2(source / "packaging/icons/io.github.sqeezy_53.school-day.svg", output / "school-day.svg")
    shutil.copy2(source / "packaging/icons/png/school-day-512.png", output / "school-day.png")
    for name in ("README.md", "packaging/RELEASE-NOTES.md"):
        if (source / name).is_file():
            shutil.copy2(source / name, output / Path(name).name)
    files = [bundle, archive, output / "SOURCE.json", output / "school-day.svg", output / "school-day.png",
             output / "README.md", output / "RELEASE-NOTES.md"]
    (output / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in files
    ))
    (work / "latest-build.txt").write_text(str(stage) + "\n")
    print(f"\nГотово: {bundle}\nИсходники: {archive}\nСборка: {stage}")


if __name__ == "__main__":
    main()
