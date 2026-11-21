#!/bin/sh
# Exercise signed installs and updates without touching installed applications.
set -eu
app=io.github.sqeezy_53.school-day
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
export FLATPAK_USER_DIR="$tmp/user" XDG_DATA_HOME="$tmp/data" XDG_CONFIG_HOME="$tmp/config" XDG_CACHE_HOME="$tmp/cache" GNUPGHOME="$tmp/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --passphrase '' --quick-generate-key 'School Day CI' ed25519 sign 1d
fingerprint=$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr"{print $10;exit}')
mkdir -p "$tmp/build/files"
cat > "$tmp/build/metadata" <<META
[Application]
name=$app
runtime=org.gnome.Platform/x86_64/51
sdk=org.gnome.Sdk/x86_64/51
command=school-day
META
mkdir -p "$tmp/build/export"
build() {
    echo "$1" > "$tmp/build/files/revision"
    flatpak build-export "$tmp/source" "$tmp/build" master
    python3 "$script_dir/prepare-repository.py" --source "$tmp/source" --destination "$tmp/site" \
        --url https://example.invalid/repo --fingerprint "$fingerprint" --keyring "$GNUPGHOME"
}
build first
sed "s|https://example.invalid/repo/|file://$tmp/site/repo/|" "$tmp/site/school-day.flatpakrepo" > "$tmp/local.flatpakrepo"
flatpak remote-add --user school-day "$tmp/local.flatpakrepo"
flatpak install --user -y --no-deps school-day "$app//master"
test "$(cat "$(flatpak info --user --show-location "$app")/files/revision")" = first
build second
flatpak update --user --no-deps -y "$app"
test "$(flatpak info --user --show-origin "$app")" = school-day
test "$(cat "$(flatpak info --user --show-location "$app")/files/revision")" = second

# A signed summary must not make an unsigned application commit trusted.
echo unsigned > "$tmp/build/files/revision"
flatpak build-export --no-update-summary "$tmp/site/repo" "$tmp/build" master
flatpak build-update-repo --gpg-sign="$fingerprint" --gpg-homedir="$GNUPGHOME" "$tmp/site/repo"
if flatpak update --user --no-deps -y "$app"; then
    echo "Unsigned update was unexpectedly accepted" >&2
    exit 1
fi
test "$(cat "$(flatpak info --user --show-location "$app")/files/revision")" = second

# The remote is unavailable. The deployed commit remains usable.
mv "$tmp/site/repo" "$tmp/site/offline"
flatpak update --user --no-deps -y "$app" || :
test "$(cat "$(flatpak info --user --show-location "$app")/files/revision")" = second
