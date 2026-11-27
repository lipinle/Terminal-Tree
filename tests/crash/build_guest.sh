#!/usr/bin/env bash
# Build an isolated RAM-only Linux guest. No physical disks or host directories
# are passed through to the VM.
set -euo pipefail
if [[ $# != 2 && $# != 3 ]]; then echo "Usage: $0 KERNEL_VERSION OUTPUT_INITRAMFS [RUST_PROBE]" >&2; exit 2; fi
version=$1
output=$(realpath -m "$2")
here=$(cd -- "$(dirname -- "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
root=$(mktemp -d)
trap 'rm -rf -- "$root"' EXIT
mkdir -p "$root"/{tt/bin,tt/modules,dev,proc,sys,mnt/data,tmp,usr/bin,usr/lib,etc}
install -m 755 "$(command -v busybox)" "$root/tt/busybox"
for app in sh mount insmod mkdir sleep cat; do ln -s ../busybox "$root/tt/bin/$app"; done
copy_binary() {
    local binary=$1 dest=$2 deps lib
    install -D -m 755 "$binary" "$root/$dest"
    deps=$(ldd "$binary")
    if grep -q 'not found' <<<"$deps"; then echo "$deps" >&2; return 1; fi
    while read -r lib; do
        [[ -n "$lib" ]] || continue
        install -D -m 755 "$lib" "$root$lib"
    done < <(awk '/=> \// {print $3} /^[[:space:]]*\/[^ ]+ \(/ {print $1}' <<<"$deps")
}
py=/usr/bin/python3
pylib=$("$py" -c 'import sysconfig; print(sysconfig.get_path("stdlib"))')
copy_binary "$py" usr/bin/python3
copy_binary /usr/bin/cp usr/bin/cp
if [[ $# == 3 ]]; then copy_binary "$(realpath "$3")" tt/tt-crash-probe; fi
cp -aL "$pylib" "$root/usr/lib/"
while IFS= read -r -d '' lib; do
    copy_binary "$lib" "${lib#/}"
done < <(find "$pylib/lib-dynload" -type f -name '*.so' -print0)
# Resolve dependencies in host kmod; unpack modules for static BusyBox insmod.
declare -A seen=()
: > "$root/tt/modules/order"
for mod in virtio_pci virtio_blk ext4; do
    deps=$(modprobe --set-version "$version" --show-depends "$mod")
    while read -r action file rest; do
        [[ "$action" == insmod ]] || continue
        [[ -z "${seen[$file]+x}" ]] || continue
        seen[$file]=1
        name=$(basename "$file")
        case "$name" in
          *.zst) name=${name%.zst}; zstd -dc -- "$file" > "$root/tt/modules/$name";;
          *.xz) name=${name%.xz}; xz -dc -- "$file" > "$root/tt/modules/$name";;
          *.gz) name=${name%.gz}; gzip -dc -- "$file" > "$root/tt/modules/$name";;
          *) cp -- "$file" "$root/tt/modules/$name";;
        esac
        echo "/tt/modules/$name" >> "$root/tt/modules/order"
    done <<<"$deps"
done
install -m 755 "$here/guest-init.sh" "$root/tt/init"
install -m 644 "$here/guest.py" "$root/tt/guest.py"
install -m 644 "$repo/legacy/python/src/tree.py" "$root/tt/tree.py"
printf 'root:x:0:0:root:/root:/tt/bin/sh\n' > "$root/etc/passwd"
printf 'root:x:0:\n' > "$root/etc/group"
mkdir -p "$(dirname "$output")"
(cd "$root" && find . -print0 | cpio --null -o -H newc --owner=0:0 --quiet | gzip -1) > "$output"
