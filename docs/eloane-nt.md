# Eloane NT : document de conception

> **Statut : piste de recherche, non démarrée.** Ce document fixe les idées pour plus tard.
> Le produit actuel reste Eloane (base Linux + Proton), voir [architecture.md](architecture.md).

## 1. Idée

Écrire de zéro un noyau en Rust, **compatible binairement avec Windows x64**, sur lequel tournent :

- les applications et jeux Windows **non modifiés** ;
- la **pile graphique Microsoft d'origine** (`dxgkrnl.sys`, `dxgmms2.sys`, `watchdog.sys`) et les
  **drivers GPU Windows des constructeurs** (NVIDIA, AMD, Intel).

Les composants Microsoft ne sont **jamais distribués** par Eloane. Ils sont récupérés depuis
le Windows de l'utilisateur, sous licence (voir section 7).

Le but est un système qui ne garde de Windows que ce qui sert au jeu : le noyau, DirectX et
les drivers. Ni explorer, ni télémétrie, ni Defender, ni Windows Update, ni la centaine de
services.

## 2. Objectifs et non-objectifs

**Objectifs**
- Charger des drivers WDDM Windows tels quels, à commencer par ceux de Microsoft
  (`BasicDisplay.sys`, `BasicRender.sys`), puis celui de la machine de référence (NVIDIA).
- Exécuter des `.exe` Windows x64 non modifiés, d'abord en console puis en Win32.
- Atteindre un rendu Direct3D 11/12 réel sur GPU.

**Non-objectifs**
- Compatibilité 32 bits (WOW64), au moins au début.
- Faire tourner les anti-cheats noyau : ils vérifient un vrai Windows signé.
- Être un Windows généraliste (bureau, impression, domaine, etc.).
- Distribuer le moindre fichier Microsoft.

## 3. Principe de compatibilité

Une application Windows n'appelle jamais le noyau directement. Elle passe par `kernel32.dll`,
puis `ntdll.dll`. Les numéros d'appels système changent à chaque build de Windows et ne sont
pas documentés. Conséquence :

- **côté applications**, la frontière de compatibilité est **l'API exportée par `ntdll`**, et
  non l'ABI des appels système. Eloane NT fournit son propre `ntdll.dll`, avec ses propres
  appels système ;
- **côté drivers**, la frontière est **l'ensemble des fonctions exportées par
  `ntoskrnl.exe` et `HAL.dll`**, ainsi que leur comportement exact (IRQL, ordre d'appel,
  structures, registre). Le noyau Eloane NT doit s'exposer sous le nom `ntoskrnl.exe` et en
  exporter les symboles.

## 4. Architecture

```
 Jeu .exe (PE x64, inchangé)
 ───────────────────────────────────────────────────────────────────────────────
 Direct3D (d3d11/d3d12/dxgi)        voie A : runtime Microsoft (fichiers de l'utilisateur)
                                    voie B : DXVK / VKD3D-Proton → ICD Vulkan du constructeur
 Driver GPU mode utilisateur        nvwgf2umx.dll, nvoglv64.dll… (paquet du constructeur)
 kernel32 / user32 / gdi32 …        DLL PE de Wine (LGPL) ; gdi32 : thunks D3DKMT* maison
 ntdll.dll                          ◄── Eloane NT : API Nt*/Rtl*/Ldr* → appels système Eloane
 ════════════════════════════════════ frontière noyau ═══════════════════════════════
 dxgkrnl.sys · dxgmms2.sys · watchdog.sys      ◄── Microsoft, fournis par l'utilisateur
 nvlddmkm.sys / BasicDisplay.sys / BasicRender.sys   ◄── drivers WDDM
 ───────────────────────────────────────────────────────────────────────────────
 Noyau Eloane NT (Rust), exposé comme « ntoskrnl.exe » + « HAL.dll »
   Ob (objets, handles) · Mm (mémoire, sections, MDL) · Ke (threads, IRQL, DPC, APC,
   attentes) · Io (IRP, pile de périphériques) · PnP · Po (énergie) · Cm (registre) ·
   Ex (pools, ressources, work items) · Etw / WMI / silos (bouchons) · chargeur PE
 ───────────────────────────────────────────────────────────────────────────────
 UEFI · ACPI · PCI/PCIe · x86-64
```

**Réutilisation**
- DLL PE de Wine (LGPL-2.1+, compatible avec notre GPL-3.0+) pour l'API Win32 ; ReactOS fait
  déjà de même.
- DXVK et VKD3D-Proton (voie B), qui évitent le runtime Direct3D de Microsoft.
- Composants Microsoft et drivers constructeurs : fournis par l'utilisateur, jamais intégrés.

## 5. Mesures de référence

Tables d'imports relevées le 2026-10-13 sur la machine de dev, Windows 11 26H2
(build 26300.9550), GPU NVIDIA RTX 2000 Ada Laptop.

| Fichier | Version | Taille | Imports |
|---|---|---|---|
| `dxgkrnl.sys` | 10.0.26100.9549 | 5,3 Mo | 590, dont 529 de ntoskrnl, 48 de watchdog, 5 de HAL |
| `dxgmms2.sys` | 10.0.26100.9549 | 1,2 Mo | 253, dont 210 de ntoskrnl et 34 de dxgkrnl |
| `watchdog.sys` | 10.0.26100.8972 | — | 106, dont 101 de ntoskrnl |
| `nvlddmkm.sys` | 32.0.15.9658 | 105 Mo | 336, dont 325 de ntoskrnl |
| `BasicDisplay.sys` | 10.0.26100.8521 | — | **66**, dont 50 de ntoskrnl |
| `BasicRender.sys` | 10.0.26100.8972 | — | **78**, dont 77 de ntoskrnl |

**Union pour la pile complète** (dxgkrnl + dxgmms2 + watchdog + nvlddmkm) : **670 fonctions
ntoskrnl distinctes et 5 de HAL.** Répartition par préfixe : Rtl 141, Ex 88, Ke 79, Io 72,
Zw 66, Ps 56, Mm 46, Po 23, Ob 22, Se 11, Etw 9.

La majorité de ces fonctions est documentée dans le WDK. Les fonctions qui révèlent un
**couplage interne à Microsoft** sont les plus risquées :

- `PsGetProcessDxgProcess` : champ du processus réservé à DirectX ;
- `PsGetCurrentThreadWin32Thread` : lien avec win32k ;
- famille `Ps*Silo*` : conteneurs ;
- `EtwpDisableStackWalkApc`, `EtwpReenableStackWalkApc` : traçage interne ;
- `KiBugCheckData` : variable exportée, pas une fonction.

Outil de mesure : un petit lecteur de tables d'imports PE, à intégrer au dépôt sous
`tools/` le jour où la piste démarre.

## 6. Sous-systèmes du noyau

| Sous-système | Contenu minimal | Remarques |
|---|---|---|
| Chargeur PE | Images noyau et utilisateur, relocations, imports par nom ou ordinal, forwarders, TLS, sections `INIT`/`PAGE` | Premier livrable |
| Ke | Threads, ordonnanceur, **IRQL**, DPC, APC, timers, objets d'attente, spinlocks, multiprocesseur | Comportement IRQL exact indispensable pour les drivers |
| Mm | Espaces d'adressage, pools paged/non-paged, MDL, sections, mapping de mémoire physique (`MmMapIoSpace`), DMA, write-combining | dxgmms2 en dépend énormément |
| Ob | Objets nommés, handles, références, espace de noms `\Device`, `\Driver`, `\BaseNamedObjects` | |
| Io | IRP, pile de périphériques, `DriverEntry`, `AddDevice`, interruptions **MSI/MSI-X** | |
| PnP / Po | Énumération PCI, `IRP_MN_START_DEVICE` avec ressources, états d'énergie D0/D3 | Point délicat pour NVIDIA |
| Cm | Registre en mémoire, persistance, ruches `SYSTEM` et `SOFTWARE`, clés de classe Display | Les drivers lisent leur configuration ici |
| Ex / Rtl | Pools, ressources, push locks, work items, chaînes, bitmaps, AVL… | Rtl réutilisable en grande partie |
| Etw / WMI / Silo / Se | Bouchons cohérents (succès neutre) | À affiner au cas par cas |
| HAL | 5 fonctions importées | Négligeable |

**Mode utilisateur**
- `ntdll` maison : `Nt*`, `Rtl*`, `Ldr*` (chargeur de DLL utilisateur), démarrage de processus.
- Chemin `D3DKMT*` : `gdi32`/`win32u` maison qui transmettent aux points d'entrée de
  `dxgkrnl`. Sous Windows, ces appels transitent par win32k ; le mécanisme exact
  d'enregistrement de dxgkrnl auprès de win32k est à étudier.
- Fenêtrage, entrées et focus : un « win32k » minimal maison, car on ne réutilise pas
  win32k de Microsoft.

## 7. Composants Windows fournis par l'utilisateur

### Sources, par ordre de préférence
1. **Partition Windows existante** de l'utilisateur, sur le modèle d'Asahi Linux qui extrait
   les firmwares depuis macOS. C'est la plus défendable : licence déjà présente sur la
   machine, souvent une licence OEM inscrite dans la table ACPI `MSDM`.
2. **ISO officielle** téléchargée sur microsoft.com et extraite avec `wimlib`. Défaut :
   seule la dernière version est disponible.
3. **Serveur de symboles Microsoft** : techniquement idéal (fichier exact, version figée),
   mais prévu pour le débogage. À éviter.
4. **Drivers GPU** : paquet officiel du constructeur (nvidia.com…), extrait avec 7-Zip.

### Manifeste de composants
```toml
# components/win11-24h2.toml
[dxgkrnl]
file    = "System32/drivers/dxgkrnl.sys"
version = "10.0.26100.9549"
sha256  = "…"                      # tout fichier non conforme est refusé
sources = ["windows-partition", "official-iso"]

[nvidia]
package = "nvidia-display-32.0.15.9658"
inf     = "nvbl.inf"
```

- **Versions figées et testées** : le noyau est validé contre une version précise de
  `dxgkrnl`, pas contre n'importe laquelle.
- **Interpréteur INF** : installer un driver revient à créer des services et des clés de
  registre, à copier des fichiers et à associer des identifiants PCI. C'est une brique à
  part entière.
- **Premier démarrage explicite** : un écran explique le besoin et propose « Depuis ma
  partition Windows » ou « Depuis une ISO ». L'action reste celle de l'utilisateur.

### Aspects juridiques (à valider par un juriste avant toute publication)
- Eloane ne distribue aucun fichier Microsoft. Seul le code qui va les chercher est publié.
  Précédents : `firmware-b43-installer` (Debian), Asahi Linux, winetricks.
- Point non résolu : la licence Windows autorise-t-elle à exécuter ses composants sur un
  autre noyau ? Il faut lire précisément le CLUF.
- Pas d'ingénierie inverse par désassemblage du code Microsoft pour en recopier
  l'implémentation. On s'appuie sur la documentation (WDK, MSDN), l'observation des
  imports et exports, et des tests de comportement.

## 8. Jalons

Les jalons 0 à 4 tournent entièrement dans QEMU, sans GPU.

| # | Jalon | Critère de sortie |
|---|---|---|
| 0 | Démarrage | Noyau Rust UEFI, mémoire, interruptions, port série, sortie framebuffer GOP |
| 1 | Chargeur de drivers | Un `.sys` de test maison (PE, imports résolus vers nos exports) exécute `DriverEntry` |
| 2 | Premier driver Microsoft | `BasicDisplay.sys` + `watchdog.sys` démarrent et affichent via le framebuffer UEFI |
| 3 | Pile DirectX noyau | `dxgkrnl.sys` + `dxgmms2.sys` s'initialisent, `BasicDisplay` s'enregistre auprès d'eux |
| 4 | Premier rendu 3D | `BasicRender.sys` + WARP : un triangle Direct3D 11 depuis un `.exe` non modifié |
| 5 | Vrai GPU | `nvlddmkm.sys` démarre sur la RTX ; un mode d'affichage est appliqué |
| 6 | Jeu | Un jeu DX11 tourne, via DXVK et l'ICD Vulkan NVIDIA ou via le runtime Microsoft |

Applications, en parallèle à partir du jalon 1 :

- **A1** : un `hello.exe` console non modifié (`ntdll` maison) ;
- **A2** : les DLL Wine se chargent, programmes Win32 console ;
- **A3** : fenêtres et entrées.

## 9. Outillage et tests

- **QEMU + OVMF** pour les jalons 0 à 4, avec port série et moniteur, comme pour Eloane Linux.
- **Tableau de bord des imports** : pour chaque driver cible, état de chaque fonction importée
  (absente, bouchon, implémentée, testée). Il sert de mesure d'avancement.
- **Protocole KD (débogueur noyau Windows) sur port série** : l'implémenter permettrait de
  déboguer Eloane NT et les drivers chargés avec **WinDbg**, l'outil naturel pour ces binaires.
- **Comparaison avec un vrai Windows** : tracer les appels d'un driver sous Windows en VM,
  puis rejouer et comparer la séquence sous Eloane NT.
- **Jalons 5-6** : passthrough GPU avec un hôte Linux (VFIO) ou un second PC dédié. Un hôte
  Windows ne sait pas passer un GPU à QEMU.

## 10. Risques

| Risque | Impact | Atténuation |
|---|---|---|
| Comportements implicites de dxgkrnl et des drivers, non documentés | Blocages longs sans visibilité | Jalons Microsoft simples d'abord, KD + WinDbg, traces comparatives |
| Évolution de WDDM à chaque version de Windows | Maintenance continue | Figer une version, manifeste de composants |
| Licence Windows | Projet non publiable en l'état | Avis juridique avant publication, sources défendables uniquement |
| Taille du driver NVIDIA (105 Mo), boîte noire | Débogage très coûteux | Commencer par BasicDisplay et BasicRender |
| Effort global | Plusieurs années | Piste de recherche non bloquante, jalons visibles |

## 11. Relation avec Eloane Linux

- Eloane Linux reste **le produit** : il fait tourner les jeux aujourd'hui.
- Eloane NT est une **piste de recherche indépendante**, dans un dossier ou un dépôt séparé
  le jour où elle démarre.
- Réutilisable des deux côtés : DXVK et VKD3D-Proton, les DLL Wine, l'outillage QEMU, la
  détection matérielle (`eloane-hw`, logique PCI).

## 12. Questions ouvertes

- Comment `dxgkrnl` s'enregistre-t-il auprès de win32k pour les appels `D3DKMT*`, et que
  faut-il fournir pour s'y substituer ?
- Quelles structures internes sont lues directement par les drivers, et non via des
  fonctions ? Ce sont des offsets à respecter.
- Voie A (runtime Direct3D Microsoft) ou voie B (DXVK + ICD Vulkan constructeur) en premier ?
- Quel build Windows figer : la version LTSC, pour sa stabilité ?
- Interpréteur INF : réutiliser celui de Wine (`setupapi`) ou en écrire un minimal ?
