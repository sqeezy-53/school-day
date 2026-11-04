#!/usr/bin/env python3
"""Build a Windows x64 installer on Linux with MinGW and locked MSYS2 libraries."""

import argparse
import concurrent.futures
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tomllib
import urllib.request

PROJECT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.sqeezy_53.school-day"


def run(command, **kwargs):
    print("+", " ".join(map(str, command)), flush=True)
    return subprocess.run(command, check=True, **kwargs)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch(url, target, expected=None):
    if target.is_file() and (not expected or digest(target) == expected):
        return target
    temporary = target.with_suffix(target.suffix + ".partial")
    with urllib.request.urlopen(url, timeout=90) as response, temporary.open("wb") as output:
        shutil.copyfileobj(response, output)
    if expected and digest(temporary) != expected:
        raise RuntimeError(f"Неверная контрольная сумма: {target.name}")
    temporary.replace(target)
    return target


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cross-bin", type=Path, required=True)
    parser.add_argument("--makensis", type=Path, required=True)
    parser.add_argument("--nsis-dir", type=Path)
    args = parser.parse_args()
    args.cross_bin = args.cross_bin.resolve()
    work, output = args.work_dir.resolve(), args.output_dir.resolve()
    for path in (work, output, work / "downloads", work / "gtk"):
        path.mkdir(parents=True, exist_ok=True)
    lock = json.loads((PROJECT / "packaging/windows/msys2-lock.json").read_text())

    def library(package):
        archive = fetch(package["binary_url"], work / "downloads" / package["FILENAME"][0], package["SHA256SUM"][0])
        run(["bsdtar", "-xf", str(archive), "-C", str(work / "gtk"), "--exclude=.*"], stdout=subprocess.DEVNULL)

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(library, lock.values()))
    prefix = work / "gtk/ucrt64"
    env = os.environ.copy()
    env.update({
        "PATH": str(args.cross_bin.resolve()) + os.pathsep + env["PATH"],
        "CARGO_TARGET_DIR": str(work / "build/target"),
        "CARGO_BUILD_JOBS": "2",
        "PKG_CONFIG_ALLOW_CROSS": "1",
        "PKG_CONFIG_SYSROOT_DIR": str(work / "gtk"),
        "PKG_CONFIG_LIBDIR": str(prefix / "lib/pkgconfig"),
        "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER": "x86_64-w64-mingw32-gcc",
    })
    run(["cargo", "build", "--release", "--locked", "--target", "x86_64-pc-windows-gnu"], cwd=PROJECT, env=env)
    version = tomllib.loads((PROJECT / "Cargo.toml").read_text())["package"]["version"]
    payload = work / "build/payload"
    if payload.exists():
        shutil.rmtree(payload)
    (payload / "bin").mkdir(parents=True)
    executable = payload / "bin/school-day.exe"
    shutil.copy2(work / "build/target/x86_64-pc-windows-gnu/release/school_app_learning.exe", executable)
    run([str(args.cross_bin / "x86_64-w64-mingw32-strip"), str(executable)])

    # Замыкаем импорты PE: системные DLL Windows не копируем из среды сборки.
    available = {path.name.lower(): path for path in (prefix / "bin").glob("*.dll")}
    queue = [executable]
    query = prefix / "bin/gdk-pixbuf-query-loaders.exe"
    shutil.copy2(query, payload / "bin" / query.name)
    queue.append(payload / "bin" / query.name)
    loader_directory = Path("lib/gdk-pixbuf-2.0/2.10.0/loaders")
    (payload / loader_directory).mkdir(parents=True)
    for name in ("pixbufloader_svg.dll", "libpixbufloader-png.dll", "libpixbufloader-bmp.dll",
                 "libpixbufloader-ico.dll", "libpixbufloader-jpeg.dll", "libpixbufloader-gif.dll"):
        loader = prefix / loader_directory / name
        destination = payload / loader_directory / loader.name
        shutil.copy2(loader, destination)
        queue.append(destination)
    copied, system = set(), set()
    while queue:
        path = queue.pop()
        details = subprocess.check_output([str(args.cross_bin / "x86_64-w64-mingw32-objdump"), "-p", str(path)], text=True)
        for name in re.findall(r"DLL Name:\s*(\S+)", details):
            key = name.lower()
            if key not in available:
                system.add(key)
                continue
            if key in copied:
                continue
            source = available[key]
            target = payload / "bin" / source.name
            shutil.copy2(source, target)
            copied.add(key)
            queue.append(target)

    for name in ("share/icons", "share/glib-2.0/schemas", "etc/fonts"):
        if (prefix / name).is_dir():
            shutil.copytree(prefix / name, payload / name, dirs_exist_ok=True)
    run(["glib-compile-schemas", str(payload / "share/glib-2.0/schemas")])
    for language in ("ru", "en", "en_GB"):
        source = prefix / "share/locale" / language
        if source.is_dir():
            shutil.copytree(source, payload / "share/locale" / language)
    icon = PROJECT / "packaging/icons/io.github.sqeezy_53.school-day.svg"
    destination = payload / f"share/icons/hicolor/scalable/apps/{APP_ID}.svg"
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(icon, destination)
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        target = payload / f"share/icons/hicolor/{size}x{size}/apps/{APP_ID}.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(PROJECT / f"packaging/icons/png/school-day-{size}.png", target)
    shutil.copytree(prefix / "share/licenses", payload / "share/licenses/msys2")
    shutil.copy2(PROJECT / "packaging/NOTICE", payload / "NOTICE")
    app_licenses = payload / "share/licenses/school-day"
    app_licenses.mkdir(parents=True, exist_ok=True)
    shutil.copy2(PROJECT / "LICENSE", app_licenses / "LICENSE")

    # Выясняем, какие пакеты предоставили включённые DLL, значки и схемы.
    supplied = {"ucrt64/" + str(p.relative_to(payload)) for p in payload.rglob("*") if p.is_file()}
    owners = {}
    for name, package in lock.items():
        archive = work / "downloads" / package["FILENAME"][0]
        members = subprocess.check_output(["bsdtar", "-tf", str(archive)], text=True).splitlines()
        if supplied.intersection(members):
            owners[name] = package
    (work / "build/included-libraries.json").write_text(json.dumps(owners, indent=2))
    (payload / "share/licenses/libraries.json").write_text(json.dumps(owners, indent=2))
    spec = importlib.util.spec_from_file_location("flatpak_packaging", PROJECT / "packaging/build-flatpak.py")
    flatpak = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(flatpak)
    with (work / "build/vendor-config.toml").open("w") as config:
        run(["cargo", "vendor", "--locked", "--versioned-dirs", str(work / "vendor")], cwd=PROJECT, stdout=config)
    flatpak.copy_licenses(work / "vendor", payload / "share/licenses/rust")

    installer = output / f"school-day-{version}-windows-x64-setup.exe"
    if args.nsis_dir:
        env["NSISDIR"] = str(args.nsis_dir.resolve())
    run([str(args.makensis.resolve()), "-V2", f"-DAPP_VERSION={version}", f"-DPAYLOAD={payload}", f"-DOUTPUT={installer}",
         str(PROJECT / "packaging/windows/installer.nsi")], cwd=PROJECT / "packaging/windows", env=env)

    # Исходники включённых библиотек поставляются отдельным приложением к релизу.
    sources = {package["source_url"] for package in owners.values()}
    def source_archive(url):
        path = fetch(url, work / "downloads" / url.rsplit("/", 1)[1])
        run(["bsdtar", "-tf", str(path)], stdout=subprocess.DEVNULL)
        return path
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        source_paths = list(pool.map(source_archive, sorted(sources)))
    libraries_archive = output / f"school-day-{version}-windows-library-sources.tar"
    with tarfile.open(libraries_archive, "w") as archive:
        for path in source_paths:
            archive.add(path, arcname=path.name)
    snapshot = work / "build/source"
    if snapshot.exists():
        shutil.rmtree(snapshot)
    snapshot.mkdir()
    for name in ("src", "packaging", "docs", "Cargo.toml", "Cargo.lock", "build.rs", "README.md", "LICENSE"):
        source = PROJECT / name
        if source.is_dir():
            shutil.copytree(source, snapshot / name, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        elif source.is_file():
            shutil.copy2(source, snapshot / name)
    shutil.copytree(work / "vendor", snapshot / "vendor")
    (snapshot / ".cargo").mkdir()
    (snapshot / ".cargo/config.toml").write_text('[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n')
    app_archive = output / f"school-day-{version}-windows-source.tar.gz"
    with tarfile.open(app_archive, "w:gz") as archive:
        archive.add(snapshot, arcname=f"school-day-{version}")
    provenance = output / "SOURCE-windows.json"
    provenance.write_text(json.dumps({
        "version": version, "target": "x86_64-pc-windows-gnu",
        "base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=PROJECT, text=True).strip(),
        "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=PROJECT)),
        "source_hashes": {str(p.relative_to(snapshot)): digest(p) for p in sorted(snapshot.rglob("*")) if p.is_file() and "vendor" not in p.relative_to(snapshot).parts},
        "included_libraries": owners, "system_dlls": sorted(system),
        "library_sources": {p.name: digest(p) for p in source_paths},
    }, ensure_ascii=False, indent=2))
    (output / "SHA256SUMS-windows").write_text("".join(
        f"{digest(path)}  {path.name}\n" for path in (installer, app_archive, libraries_archive, provenance)
    ))
    print(f"Готово: {installer}; DLL: {len(copied)}; пакетов с исходниками: {len(sources)}")


if __name__ == "__main__":
    main()
