#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <x86_64|aarch64> <output-directory>" >&2
  exit 2
fi

architecture=$1
output_directory=$(realpath -m "$2")
repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
spec="$repository_root/packaging/fedora/rayslash-module-host.spec"
checksums="$repository_root/packaging/fedora/release-sources.sha256"
version=$(rpm --specfile "$spec" --qf '%{version}\n' | head -n1)

case "$architecture" in
  x86_64) target=x86_64-unknown-linux-gnu ;;
  aarch64) target=aarch64-unknown-linux-gnu ;;
  *) echo "unsupported architecture: $architecture" >&2; exit 2 ;;
esac

archive="rayslash-module-host-v${version}-${target}.tar.xz"
sidecar="${archive}.sha256"
tag="v${version}"
release_base="https://github.com/rslauncher/rayslash-module-host/releases/download/${tag}"
work_directory=$(mktemp -d)
trap 'rm -rf "$work_directory"' EXIT
sources="$work_directory/sources"
topdir="$work_directory/rpmbuild"
mkdir -p "$sources" "$topdir"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS} "$output_directory"

curl --fail --location --retry 3 --output "$sources/$archive" "$release_base/$archive"
curl --fail --location --retry 3 --output "$sources/$sidecar" "$release_base/$sidecar"
curl --fail --location --retry 3 --output "$sources/LICENSE" \
  "https://raw.githubusercontent.com/rslauncher/rayslash-module-host/${tag}/LICENSE"

for source in LICENSE "$archive" "$sidecar"; do
  expected=$(awk -v name="$source" '$2 == name { print $1 }' "$checksums")
  if [[ -z "$expected" ]]; then
    echo "missing pinned checksum for $source" >&2
    exit 1
  fi
  printf '%s  %s\n' "$expected" "$source" | (cd "$sources" && sha256sum --check --strict -)
done
(cd "$sources" && sha256sum --check --strict "$sidecar")

cp "$sources/$archive" "$sources/LICENSE" "$topdir/SOURCES/"
cp "$spec" "$topdir/SPECS/"
source_date_epoch=$(git -C "$repository_root" log -1 --format=%ct "$tag")
SOURCE_DATE_EPOCH=$source_date_epoch \
  rpmbuild -bb --target "$architecture" \
    --define "_topdir $topdir" \
    --define "_buildhost build.rayslash.invalid" \
    --define "use_source_date_epoch_as_buildtime 1" \
    --define "clamp_mtime_to_source_date_epoch 1" \
    "$topdir/SPECS/rayslash-module-host.spec"

rpm_path=$(find "$topdir/RPMS/$architecture" -maxdepth 1 -type f -name '*.rpm' -print -quit)
if [[ -z "$rpm_path" ]]; then
  echo "rpmbuild did not produce an RPM for $architecture" >&2
  exit 1
fi

mkdir "$work_directory/rpm-root"
(cd "$work_directory/rpm-root" && rpm2cpio "$rpm_path" | cpio --quiet --extract --make-directories)
tar --extract --to-stdout --file "$sources/$archive" \
  "rayslash-module-host-v${version}-${target}/rayslash-module-host" > \
  "$work_directory/released-host"
cmp "$work_directory/released-host" \
  "$work_directory/rpm-root/usr/libexec/rayslash/rayslash-module-host"

cp "$rpm_path" "$output_directory/"
rpm_name=$(basename "$rpm_path")
(cd "$output_directory" && sha256sum "$rpm_name" > "$rpm_name.sha256")
