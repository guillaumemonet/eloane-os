# Eloane

Eloane est un système d'exploitation dédié au jeu : il fait tourner les jeux Windows x64
**sans Windows**, sur un socle Linux minimal, sans bureau ni surcouche inutile.

Au démarrage, la machine arrive directement dans une session plein écran
(gamescope + Steam Big Picture). Les jeux Windows passent par Proton/Wine, DirectX est
traduit en Vulkan (DXVK, VKD3D-Proton) et rendu par les drivers GPU natifs
(Mesa pour AMD/Intel, `nvidia-open` pour NVIDIA).

> Statut : pré-alpha. Premier jalon : une ISO live UEFI qui boote dans Steam.

## Architecture en bref

```
┌──────────────────────────────────────────────────────────┐
│  Jeu Windows (.exe, DirectX 9-12)                        │
├──────────────────────────────────────────────────────────┤
│  Proton / Wine   ·   DXVK / VKD3D-Proton   ·   Steam     │
├──────────────────────────────────────────────────────────┤
│  gamescope (compositeur)  ◄── eloane-session (Rust)      │
├──────────────────────────────────────────────────────────┤
│  Mesa / NVIDIA (Vulkan)  ·  PipeWire  ·  NetworkManager  │
├──────────────────────────────────────────────────────────┤
│  Linux + systemd (base Arch Linux)                       │
├──────────────────────────────────────────────────────────┤
│  Matériel PC x86-64, boot UEFI                           │
└──────────────────────────────────────────────────────────┘
```

Détails : [docs/architecture.md](docs/architecture.md). Feuille de route : [docs/roadmap.md](docs/roadmap.md).

## Arborescence

| Chemin       | Contenu                                                        |
|--------------|----------------------------------------------------------------|
| `crates/`    | Composants Rust : `eloane-hw`, `eloane-session`, `eloane-ctl`  |
| `image/`     | Profil archiso (paquets, fichiers du système, boot UEFI)       |
| `build/`     | Conteneur de build et scripts de génération de l'ISO           |
| `scripts/`   | Outils de développement (lancement dans QEMU…)                 |
| `docs/`      | Architecture, feuille de route, décisions                      |

## Construire l'ISO (depuis Windows)

Prérequis : Docker Desktop (backend WSL2). Rien d'autre à installer sur l'hôte.

```powershell
.\build\build.ps1            # build complet (composants Rust + ISO)
.\build\build.ps1 -SkipRust  # ISO seule
```

L'ISO est produite dans `out/`. Pour la tester dans QEMU :

```powershell
winget install SoftwareFreedomConservancy.QEMU
.\scripts\run-qemu.ps1
```

QEMU sert à vérifier le boot et la session. Les performances de jeu se mesurent sur du
vrai matériel (clé USB bootable).

## Limites connues

- **Anti-cheat noyau** (Vanguard, certains modes EAC/BattlEye) : ces jeux ne tournent sur
  aucun OS autre que Windows. Eloane n'y changera rien.
- Compatibilité des jeux : voir [ProtonDB](https://www.protondb.com/).

## Licence

GPL-3.0-or-later, voir [LICENSE](LICENSE). Elle permet d'intégrer et de modifier du code
issu de Wine (LGPL-2.1+), ReactOS (GPL-2.0+), Mesa (MIT) et de l'écosystème Rust
(MIT/Apache-2.0).
