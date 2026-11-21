#!/usr/bin/env python3
"""Sign a local build into a staging copy of the distribution repository."""
import argparse
import base64
from pathlib import Path
import re
import subprocess
from urllib.parse import urlsplit

APP = "io.github.sqeezy_53.school-day"
BRANCH = "master"
REF = f"app/{APP}/x86_64/{BRANCH}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--url", required=True, help="Public HTTPS URL of the repo directory")
    parser.add_argument("--fingerprint", required=True)
    parser.add_argument("--keyring", type=Path, required=True)
    args = parser.parse_args()
    url = urlsplit(args.url)
    if url.scheme != "https" or not url.hostname or url.username or url.password or url.query or url.fragment or any(c.isspace() for c in args.url):
        parser.error("Нужен HTTPS-адрес каталога репозитория")
    if not re.fullmatch(r"[A-Fa-f0-9]{40}|[A-Fa-f0-9]{64}", args.fingerprint):
        parser.error("Нужен полный отпечаток ключа")
    root = args.destination.resolve()
    repo = root / "repo"
    if repo == args.source.resolve():
        parser.error("Каталог источника должен отличаться от каталога назначения")
    key = subprocess.check_output(["gpg", "--homedir", args.keyring, "--export", args.fingerprint])
    if not key:
        parser.error("Открытый ключ отсутствует")
    root.mkdir(parents=True, exist_ok=True)
    if not (repo / "config").exists():
        subprocess.run(["ostree", "init", f"--repo={repo}", "--mode=archive-z2"], check=True)
    sign = [f"--gpg-homedir={args.keyring}", f"--gpg-sign={args.fingerprint}"]
    subprocess.run(["flatpak", "build-commit-from", f"--src-repo={args.source}", f"--src-ref={REF}",
                    "--no-update-summary", *sign, repo, REF], check=True)
    subprocess.run(["flatpak", "build-update-repo", "--title=School Day", "--default-branch=master",
                    "--generate-static-deltas", "--static-delta-jobs=1", *sign, repo], check=True)
    subprocess.run(["ostree", "fsck", f"--repo={repo}"], check=True)
    encoded = base64.b64encode(key).decode("ascii")
    common = f"Title=School Day\nUrl={args.url.rstrip('/')}/\nGPGKey={encoded}\n"
    (root / "school-day.flatpakrepo").write_text("[Flatpak Repo]\nVersion=1\n" + common)
    (root / "school-day.flatpakref").write_text("[Flatpak Ref]\n" + common + f"Name={APP}\nBranch={BRANCH}\n"
        "IsRuntime=false\nSuggestRemoteName=school-day\nRuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo\n")


if __name__ == "__main__":
    main()
