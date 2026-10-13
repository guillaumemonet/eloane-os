# Feuille de route

## Jalon 0 : Fondations *(en cours)*
- [ ] Repo, licence, doc d'architecture
- [ ] Workspace Rust : `eloane-hw`, `eloane-session`, `eloane-ctl` + tests
- [ ] Profil archiso UEFI + conteneur de build Docker
- [ ] ISO live qui boote dans QEMU jusqu'à la session gamescope

## Jalon 1 : Premier jeu
- [ ] Boot sur la machine de dev (hybride Intel + RTX 2000 Ada) depuis clé USB
- [ ] Steam Big Picture fonctionnel : réseau, son, manette
- [ ] Un jeu DX11 (DXVK) et un jeu DX12 (VKD3D-Proton) lancés via Proton sur le dGPU
- [ ] Mesure de FPS comparée à Windows sur les mêmes jeux (MangoHud)

## Jalon 2 : Installation et persistance
- [ ] Installeur minimal (disque entier, partition EFI + système + données)
- [ ] Persistance de la bibliothèque Steam et des sauvegardes
- [ ] `eloane-ctl` : réglages (résolution, GPU préféré, Wi-Fi)

## Jalon 3 : Système immuable
- [ ] Image système en lecture seule, partitions A/B
- [ ] Mises à jour atomiques avec retour arrière automatique en cas d'échec de boot
- [ ] Builds signés, Secure Boot (shim/MOK pour les modules NVIDIA)

## Jalon 4 : Optimisation
- [ ] Kernel tuné jeu (ordonnanceur, préemption, NTSYNC pour Wine)
- [ ] Temps de boot jusqu'à la session < 10 s
- [ ] Lanceurs hors Steam (Heroic, Lutris) en option

## Piste de recherche (long terme, non bloquante)
- Noyau Rust exposant l'ABI Linux, pour réutiliser Mesa, Wine et le userland existant.
