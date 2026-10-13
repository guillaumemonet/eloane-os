#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Eloane - profil archiso (derive de configs/releng d'archiso 91).
# shellcheck disable=SC2034

iso_name="eloane"
iso_label="ELOANE_$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y%m)"
iso_publisher="Eloane"
iso_application="Eloane Live (Steam / Proton gaming)"
iso_version="$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)"
install_dir="eloane"
buildmodes=('iso')
# UEFI uniquement : pas de syslinux/BIOS, pas de GRUB.
bootmodes=('uefi.systemd-boot')
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'zstd' '-Xcompression-level' '15' '-b' '1M')
# Parametres communs a toutes les entrees systemd-boot (%KERNEL_PARAMS%) :
#  - cow_spacesize : l'overlay en RAM du live doit pouvoir accueillir Steam
#    (bootstrap ~1 Go) et quelques mises a jour ; 256M par defaut chez archiso.
#  - copytoram=n : on garde la RAM pour les jeux.
kernel_params_x86_64="cow_spacesize=75% copytoram=n"
file_permissions=(
  ["/etc/shadow"]="0:0:400"
  ["/etc/gshadow"]="0:0:400"
  ["/etc/sudoers.d"]="0:0:750"
  ["/etc/sudoers.d/10-eloane-live"]="0:0:440"
  ["/root"]="0:0:750"
  ["/home/gamer"]="1000:1000:750"
  ["/home/gamer/.bash_profile"]="1000:1000:644"
  ["/home/gamer/.bashrc"]="1000:1000:644"
)
# Les binaires eloane-session / eloane-ctl sont injectes par build/build-iso.sh
# (copie dans airootfs/usr/bin + ajout de leurs entrees ici).
