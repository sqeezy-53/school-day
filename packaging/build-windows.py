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
import tempfile
import tomllib
import urllib.request

PROJECT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.sqeezy_53.school-day"
ZLIB = {
    "version": "1.4",
    "source_url": "https://github.com/madler/zlib/releases/download/v1.4/zlib-1.4.tar.xz",
    "source_sha256": "4f11c512053d7bab8d9e9b89bde3575add3e4c50a383b42ef6cb837035fc5a7c",
}


def run(command, **kwargs):
    print("+", " ".join(map(str, command)), flush=True)
    return subprocess.run(command, check=True, **kwargs)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch(url, target, expected):
    if not re.fullmatch(r"[a-f0-9]{64}", expected):
        raise ValueError(f"Не задан SHA-256 для {target.name}")
    if target.is_file() and digest(target) == expected:
        return target
    with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as output:
        temporary = Path(output.name)
    try:
        with urllib.request.urlopen(url, timeout=90) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output)
        if digest(temporary) != expected:
            raise RuntimeError(f"Неверная контрольная сумма: {target.name}")
        temporary.replace(target)
    finally:
        temporary.unlink(missing_ok=True)
    return target


def build_zlib(stage, downloads, env, cross_prefix):
    url = ZLIB["source_url"]
    archive = fetch(url, downloads / url.rsplit("/", 1)[1], ZLIB["source_sha256"])
    run(["bsdtar", "-xf", str(archive), "-C", str(stage)])
    source = stage / f"zlib-{ZLIB['version']}"
        loc = "" if cross_prefix.endswith("ucrt-") else "-mcrtdll=ucrt"
    run(["make", "-f", "win32/Makefile.gcc", "-j2", f"PREFIX={cross_prefix}",
         f"LOC={loc}", "SHARED_MODE=1", f"DESTDIR={stage / 'gtk'}",
         "prefix=/ucrt64", "BINARY_PATH=/ucrt64/bin", "LIBRARY_PATH=/ucrt64/lib",
         "INCLUDE_PATH=/ucrt64/include", "install"], cwd=source, env=env)
    license_path = stage / "gtk/ucrt64/share/licenses/zlib"
    license_path.mkdir(parents=True)
    shutil.copy2(source / "LICENSE", license_path / "LICENSE")

def build_zlib(stage, downloads, env, args.cross_prefix):
    loc = "" if cross_prefix.endswith("ucrt-") else "-mcrtdll=ucrt"

def write_installer_inventory(payload, destination, loaders_cache):
    def installed_path(path):
        relative = str(path.relative_to(payload)).replace("/", "\\")
        if any(char in relative for char in '"\r\n*?'):
            raise ValueError(f"Недопустимый путь в установщике: {relative}")
        return '"$INSTDIR\\' + relative.replace("$", "$$") + '"'

    files, directories = [], []
    for path in sorted(payload.rglob("*")):
        if path.is_dir():
            directories.append(path)
        elif path.is_file():
            files.append(path)
    files.append(payload / loaders_cache)
    directories.sort(key=lambda path: len(path.parts), reverse=True)
    commands = {
        "CHECK_PAYLOAD_DIRS": [f"!insertmacro CHECK_DIRECTORY {installed_path(path)}" for path in directories],
        "CHECK_PAYLOAD_COLLISIONS": [f"IfFileExists {installed_path(path)} occupied_install_dir 0" for path in files],
        "DELETE_PAYLOAD_FILES": [f"Delete {installed_path(path)}" for path in files],
        "REMOVE_PAYLOAD_DIRS": [f"RMDir {installed_path(path)}" for path in directories],
    }
    with destination.open("w", encoding="utf-8-sig") as output:
        for macro, lines in commands.items():
            output.write(f"!macro {macro}\n")
            for line in lines:
                
                output.write(f"  {line}\n")
            output.write("!macroend\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cross-bin", type=Path, required=True)
    parser.add_argument("--cross-prefix", default="x86_64-w64-mingw32-")
    parser.add_argument("--makensis", type=Path, required=True)
    parser.add_argument("--nsis-dir", type=Path)
    args = parser.parse_args()
    args.cross_bin = args.cross_bin.resolve()
    work, output = args.work_dir.resolve(), args.output_dir.resolve()
    for path in (output, work / "downloads"):
        path.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="build-", dir=work) as stage:
        with tempfile.TemporaryDirectory(prefix="artifacts-", dir=output) as products:
            build(args, work / "downloads", Path(stage), Path(products))
            for product in Path(products).iterdir():
                product.replace(output / product.name)
    print(f"Готово: {output}")


def build(args, downloads, stage, output):
    lock = json.loads((PROJECT / "packaging/windows/msys2-lock.json").read_text())
    gtk = stage / "gtk"
    gtk.mkdir()

    def library(package):
        archive = fetch(package["binary_url"], downloads / package["FILENAME"][0], package["SHA256SUM"][0])
        run(["bsdtar", "-xf", str(archive), "-C", str(gtk),
             "--exclude=.BUILDINFO", "--exclude=.MTREE", "--exclude=.PKGINFO",
             "--exclude=.INSTALL", "--exclude=.CHANGELOG"], stdout=subprocess.DEVNULL)

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(library, lock.values()))
    prefix = gtk / "ucrt64"
    env = os.environ.copy()
    env.update({
        "PATH": str(args.cross_bin.resolve()) + os.pathsep + env["PATH"],
        "CARGO_TARGET_DIR": str(stage / "target"),
        "CARGO_BUILD_JOBS": "2",
        "PKG_CONFIG_ALLOW_CROSS": "1",
        "PKG_CONFIG_SYSROOT_DIR": str(gtk),
        "PKG_CONFIG_LIBDIR": str(prefix / "lib/pkgconfig"),
        "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER": f"{args.cross_prefix}gcc",
        "WINDRES": f"{args.cross_prefix}windres",
    })
    build_zlib(stage, downloads, env, args.cross_prefix)
    run(["cargo", "build", "--release", "--locked", "--features", "windows-updater", "--target", "x86_64-pc-windows-gnu"], cwd=PROJECT, env=env)
    version = tomllib.loads((PROJECT / "Cargo.toml").read_text())["package"]["version"]
    payload = stage / "payload"
    (payload / "bin").mkdir(parents=True)
    executable = payload / "bin/school-day.exe"
    shutil.copy2(stage / "target/x86_64-pc-windows-gnu/release/school_app_learning.exe", executable)
    run([str(args.cross_bin / f"{args.cross_prefix}strip"), str(executable)])

    # Замыкаем импорты PE: системные DLL Windows не копируем из среды сборки.
    available = {path.name.lower(): path for path in (prefix / "bin").glob("*.dll")}
    updater = payload / "bin/school-day-updater.exe"
    shutil.copy2(stage / "target/x86_64-pc-windows-gnu/release/school-day-updater.exe", updater)
    run([str(args.cross_bin / f"{args.cross_prefix}strip"), str(updater)])
    queue = [executable, updater]
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
        details = subprocess.check_output([str(args.cross_bin / f"{args.cross_prefix}objdump"), "-p", str(path)], text=True)
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
        archive = downloads / package["FILENAME"][0]
        members = subprocess.check_output(["bsdtar", "-tf", str(archive)], text=True).splitlines()
        if supplied.intersection(members):
            owners[name] = package
    owners["zlib"] = ZLIB
    (payload / "share/licenses/libraries.json").write_text(json.dumps(owners, indent=2))
    spec = importlib.util.spec_from_file_location("flatpak_packaging", PROJECT / "packaging/build-flatpak.py")
    flatpak = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(flatpak)
    with (stage / "vendor-config.toml").open("w") as config:
        run(["cargo", "vendor", "--locked", "--versioned-dirs", str(stage / "vendor")], cwd=PROJECT, stdout=config)
    flatpak.copy_licenses(stage / "vendor", payload / "share/licenses/rust")

    installer = output / f"school-day-{version}-windows-x64-setup.exe"
    if args.nsis_dir:
        env["NSISDIR"] = str(args.nsis_dir.resolve())
    inventory = stage / "inventory.nsh"
    write_installer_inventory(payload, inventory, loader_directory.parent / "loaders.cache")
    run([str(args.makensis.resolve()), "-V2", f"-DAPP_VERSION={version}", f"-DPAYLOAD={payload}",
         f"-DINVENTORY={inventory}", f"-DOUTPUT={installer}",
         str(PROJECT / "packaging/windows/installer.nsi")], cwd=PROJECT / "packaging/windows", env=env)

    # Исходники включённых библиотек поставляются отдельным приложением к релизу.
    sources = {package["source_url"]: package["source_sha256"] for package in owners.values()}
    def source_archive(item):
        url, checksum = item
        path = fetch(url, downloads / url.rsplit("/", 1)[1], checksum)
        run(["bsdtar", "-tf", str(path)], stdout=subprocess.DEVNULL)
        return path
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        source_paths = list(pool.map(source_archive, sorted(sources.items())))
    libraries_archive = output / f"school-day-{version}-windows-library-sources.tar"
    with tarfile.open(libraries_archive, "w") as archive:
        for path in source_paths:
            archive.add(path, arcname=path.name)
    snapshot = stage / "source"
    snapshot.mkdir()
    for name in ("src", "packaging", "docs", "Cargo.toml", "Cargo.lock", "build.rs", "README.md", "LICENSE"):
        source = PROJECT / name
        if source.is_dir():
            shutil.copytree(source, snapshot / name, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        elif source.is_file():
            shutil.copy2(source, snapshot / name)
    shutil.copytree(stage / "vendor", snapshot / "vendor")
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
