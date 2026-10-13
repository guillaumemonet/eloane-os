# Architecture d'Eloane

## 1. Objectif et non-objectifs

**Objectif :** faire tourner correctement les jeux Windows x64 sur PC, avec le moins de
couches possible entre le jeu et le matériel, sans Windows.

**Non-objectifs (pour l'instant) :**
- Être un OS de bureau généraliste.
- Charger des drivers Windows (`.sys`). Les drivers GPU Windows (WDDM) dépendent de
  `dxgkrnl` et de tout le noyau NT ; ReactOS n'y arrive pas après 25 ans. Les drivers
  Linux natifs sont meilleurs et maintenus par les constructeurs.
- Faire tourner les jeux à anti-cheat noyau : techniquement impossible hors Windows.

## 2. Décisions (ADR condensés)

| # | Décision | Raison | Alternatives écartées |
|---|----------|--------|-----------------------|
| 1 | Noyau Linux, pas de noyau maison | Seule voie vers des drivers GPU modernes (amdgpu, i915/xe, nvidia-open) | Noyau Rust compatible NT : des décennies avant un jeu jouable |
| 2 | Base Arch Linux | Paquets récents (Mesa, kernel), multilib 32 bits requis par Steam/Wine, même socle que SteamOS 3 | Alpine (musl, incompatible Steam), Debian (paquets graphiques trop anciens), Buildroot/Yocto (multilib + Mesa + Wine à maintenir soi-même) |
| 3 | systemd conservé | Init éprouvé, exigé de fait par logind/PipeWire/NetworkManager ; aucun gain pour les jeux à le remplacer | Init maison en Rust |
| 4 | gamescope comme seul compositeur | Conçu pour le jeu (Valve), plein écran, upscaling FSR, gestion VRR/HDR, pas de bureau | Session Wayland complète (KDE/GNOME) |
| 5 | Proton (via Steam) pour la compat Windows | Meilleure compat du marché, maintenu par Valve | Wine brut seul |
| 6 | Code maison en Rust | Sûreté mémoire, binaires statiques simples à embarquer | C, scripts shell |
| 7 | Boot UEFI uniquement | Tout PC de jeu actuel est UEFI ; simplifie le bootloader (systemd-boot) | BIOS legacy |
| 8 | Build en conteneur Docker depuis Windows | Aucun outil Linux à installer sur l'hôte, reproductible pour les contributeurs | Build natif dans WSL |
| 9 | GPL-3.0-or-later | Compatible avec Wine, ReactOS, Mesa et les crates Rust | MIT/Apache (empêche la réutilisation de code GPL/LGPL) |

## 3. Couches

```
 Jeu Windows ──► Proton/Wine (PE → appels Linux) ──► DXVK / VKD3D-Proton (D3D → Vulkan)
     │
     ▼
 gamescope (compositeur Vulkan, fenêtre unique, FSR, VRR)
     ▲ lancé et supervisé par
 eloane-session ──utilise──► eloane-hw (détection GPU, PRIME)
     │
 Mesa (RADV, ANV) / NVIDIA ── DRM/KMS noyau ── GPU
 PipeWire ── ALSA ── carte son
 systemd · logind · NetworkManager · bluez
 Linux (Arch) ── systemd-boot ── UEFI
```

## 4. Composants maison

| Crate | Rôle |
|-------|------|
| `eloane-hw` | Bibliothèque de détection matérielle via sysfs : GPU (vendor, device, driver, intégré/discret), machine hybride, GPU de rendu préféré, variables PRIME offload. |
| `eloane-session` | Point d'entrée de la session sur tty1 : détecte le GPU, construit et supervise `gamescope -- steam -gamepadui`, relance en cas de crash, rend la main au shell après N échecs. Config : `/etc/eloane/session.toml`. |
| `eloane-ctl` | Outil de diagnostic : `gpu`, `env`, `info`. Destiné à grossir (mises à jour, réglages). |

## 5. Démarrage

1. Firmware UEFI, puis systemd-boot, puis kernel `linux` (`nvidia_drm.modeset=1`).
2. systemd lance NetworkManager, PipeWire (session utilisateur) et logind.
3. getty@tty1 connecte automatiquement l'utilisateur `gamer`.
4. `.bash_profile` lance `eloane-session` sur tty1.
5. `eloane-session` démarre gamescope, qui démarre Steam en mode Big Picture.

## 6. Cas des machines hybrides (iGPU + dGPU)

C'est le cas de la machine de dev : Intel UHD + NVIDIA RTX 2000 Ada. L'écran interne est
câblé sur l'iGPU. `eloane-hw` choisit le dGPU pour le rendu et `eloane-session` passe à
gamescope `--prefer-vk-device` ainsi que les variables PRIME render offload. Le dGPU
calcule, l'iGPU affiche.

## 7. Tests

- **QEMU + OVMF** : vérifie le boot UEFI, l'autologin et le lancement de session
  (rendu logiciel lavapipe). Ne mesure pas les performances.
- **Matériel réel** : clé USB live, la machine de dev servant de référence hybride NVIDIA.
- **Rust** : tests unitaires de `eloane-hw` sur de faux arbres sysfs ; `cargo clippy -D warnings`.

## 8. Évolutions prévues

Voir [roadmap.md](roadmap.md) : installation sur disque, image système immuable A/B
avec mises à jour atomiques, kernel optimisé jeu, et à plus long terme une piste de
recherche sur un noyau Rust exposant l'ABI Linux.
