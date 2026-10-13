#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Eloane - lance l'ISO la plus recente de out/ dans QEMU (Linux / WSL, UEFI/OVMF).
#
#   scripts/run-qemu.sh [chemin.iso]
#   MEMORY=6G CPUS=2 scripts/run-qemu.sh
#
# KVM si /dev/kvm est accessible, sinon TCG (lent).
set -euo pipefail

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${REPO}/out"
MEMORY="${MEMORY:-8G}"
CPUS="${CPUS:-4}"

QEMU="$(command -v qemu-system-x86_64 || true)"
if [[ -z "${QEMU}" ]]; then
    echo "qemu-system-x86_64 introuvable. Installer QEMU :" >&2
    echo "  Arch : pacman -S qemu-desktop edk2-ovmf   Debian/Ubuntu : apt install qemu-system-x86 ovmf" >&2
    exit 1
fi

# --- ISO ----------------------------------------------------------------------
ISO="${1:-}"
if [[ -z "${ISO}" ]]; then
    ISO="$(ls -1t "${OUT_DIR}"/eloane-*.iso 2>/dev/null | head -n1 || true)"
    [[ -n "${ISO}" ]] || { echo "Aucune ISO eloane-*.iso dans ${OUT_DIR} (build/build-iso.sh d'abord)." >&2; exit 1; }
fi
[[ -f "${ISO}" ]] || { echo "ISO introuvable : ${ISO}" >&2; exit 1; }

# --- OVMF : couples code/vars selon les distributions ----------------------------
OVMF_CODE="" OVMF_VARS_TEMPLATE=""
for pair in \
    "/usr/share/OVMF/OVMF_CODE_4M.fd:/usr/share/OVMF/OVMF_VARS_4M.fd" \
    "/usr/share/OVMF/OVMF_CODE.fd:/usr/share/OVMF/OVMF_VARS.fd" \
    "/usr/share/edk2/x64/OVMF_CODE.4m.fd:/usr/share/edk2/x64/OVMF_VARS.4m.fd" \
    "/usr/share/edk2/x64/OVMF_CODE.fd:/usr/share/edk2/x64/OVMF_VARS.fd" \
    "/usr/share/edk2/ovmf/OVMF_CODE.fd:/usr/share/edk2/ovmf/OVMF_VARS.fd" \
    "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd:/usr/share/edk2-ovmf/x64/OVMF_VARS.fd" \
    "/usr/share/qemu/edk2-x86_64-code.fd:/usr/share/qemu/edk2-i386-vars.fd"; do
    code="${pair%%:*}" vars="${pair#*:}"
    if [[ -f "${code}" ]]; then OVMF_CODE="${code}" OVMF_VARS_TEMPLATE="${vars}"; break; fi
done
[[ -n "${OVMF_CODE}" ]] || { echo "OVMF introuvable (/usr/share/OVMF ou /usr/share/edk2). Installer ovmf / edk2-ovmf." >&2; exit 1; }

mkdir -p -- "${OUT_DIR}"
VARS="${OUT_DIR}/ovmf-vars.$(basename -- "${OVMF_CODE}" | tr -c 'a-zA-Z0-9.\n' '_')"
if [[ ! -f "${VARS}" && -f "${OVMF_VARS_TEMPLATE}" ]]; then
    cp -- "${OVMF_VARS_TEMPLATE}" "${VARS}"
fi

# --- Accelerateur ---------------------------------------------------------------
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    ACCEL=(-accel kvm -cpu host)
else
    echo "Avertissement : /dev/kvm indisponible, emulation TCG (tres lente)." >&2
    ACCEL=(-accel tcg,thread=multi -cpu max)
fi

ARGS=(
    -name eloane
    -machine q35
    "${ACCEL[@]}"
    -m "${MEMORY}"
    -smp "${CPUS}"
    -drive "if=pflash,format=raw,unit=0,readonly=on,file=${OVMF_CODE}"
)
[[ -f "${VARS}" ]] && ARGS+=(-drive "if=pflash,format=raw,unit=1,file=${VARS}")
ARGS+=(
    -cdrom "${ISO}"
    -boot d
    -device virtio-vga
    -device qemu-xhci
    -device usb-tablet
    -device usb-kbd
    -nic user,model=virtio-net-pci
)
# Audio si un backend est disponible (pipewire > pulseaudio), sinon rien.
if "${QEMU}" -audiodev help 2>/dev/null | grep -qx pipewire; then
    ARGS+=(-audiodev pipewire,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0)
elif "${QEMU}" -audiodev help 2>/dev/null | grep -qx pa; then
    ARGS+=(-audiodev pa,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0)
fi

echo "ISO  : ${ISO}"
echo "OVMF : ${OVMF_CODE}"
echo "==> ${QEMU} ${ARGS[*]}"
exec "${QEMU}" "${ARGS[@]}"
