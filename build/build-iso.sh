#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Eloane - construit l'ISO live. S'execute DANS le conteneur eloane-builder,
# avec le depot monte en /src (voir build/build.ps1) :
#
#   docker run --rm --privileged -v "<repo>:/src" eloane-builder bash /src/build/build-iso.sh [--skip-rust]
#
# Etapes : cargo build --release du workspace -> copie du profil image/ dans un
# dossier temporaire -> liens symboliques (image/symlinks.list) -> injection
# des binaires dans airootfs/usr/bin -> mkarchiso.
set -euo pipefail

REPO="${ELOANE_SRC:-/src}"
OUT_DIR="${REPO}/out"
WORK_DIR="/tmp/eloane-work"
PROFILE_DIR="/tmp/eloane-profile"
SKIP_RUST=0
KEEP_WORK=0
BINARIES=(eloane-session eloane-ctl)

usage() {
    cat <<EOF
Usage: $0 [--skip-rust] [--out DIR] [--work DIR] [--keep-work]

  --skip-rust   ne compile pas le workspace Rust (ISO sans eloane-session/eloane-ctl)
  --out DIR     dossier de sortie de l'ISO (defaut : ${OUT_DIR})
  --work DIR    dossier de travail mkarchiso (defaut : ${WORK_DIR})
  --keep-work   ne supprime pas le dossier de travail a la fin
EOF
}

while (( $# )); do
    case "$1" in
        --skip-rust) SKIP_RUST=1 ;;
        --out) OUT_DIR="$2"; shift ;;
        --work) WORK_DIR="$2"; shift ;;
        --keep-work) KEEP_WORK=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Option inconnue : $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

log() { printf '\n==> %s\n' "$*"; }
die() { printf '\nERREUR : %s\n' "$*" >&2; exit 1; }

START=$(date +%s)

[[ $EUID -eq 0 ]] || die "mkarchiso doit tourner en root (conteneur --privileged)."
[[ -f "${REPO}/image/profiledef.sh" ]] || die "profil introuvable : ${REPO}/image/profiledef.sh (depot monte en ${REPO} ?)"
command -v mkarchiso >/dev/null || die "mkarchiso absent : lancer ce script dans l'image eloane-builder."

# --- 1. Workspace Rust ---------------------------------------------------------
TARGET_DIR="${CARGO_TARGET_DIR:-${REPO}/target}"
if (( SKIP_RUST )); then
    log "Compilation Rust ignoree (--skip-rust)"
else
    [[ -f "${REPO}/Cargo.toml" ]] || die "Cargo.toml absent a la racine du depot (${REPO}).
Le workspace Rust (eloane-session, eloane-ctl) n'existe pas encore :
relancer avec --skip-rust (build.ps1 -SkipRust) pour construire l'ISO sans."
    log "cargo build --release (target : ${TARGET_DIR})"
    (cd "${REPO}" && cargo build --release --target-dir "${TARGET_DIR}" \
        $(printf -- '--bin %s ' "${BINARIES[@]}"))
    for b in "${BINARIES[@]}"; do
        [[ -x "${TARGET_DIR}/release/${b}" ]] || die "binaire attendu absent : ${TARGET_DIR}/release/${b}"
    done
fi

# --- 2. Copie du profil -------------------------------------------------------
log "Preparation du profil dans ${PROFILE_DIR}"
rm -rf -- "${PROFILE_DIR}"
mkdir -p -- "${PROFILE_DIR}"
cp -a -- "${REPO}/image/." "${PROFILE_DIR}/"

# Garde-fou fins de ligne : un checkout Windows sans .gitattributes donnerait du
# CRLF, fatal pour les scripts/units. On normalise la copie (fichiers texte).
while IFS= read -r -d '' f; do
    if grep -Iq . "$f" && grep -q $'\r' "$f"; then
        sed -i 's/\r$//' "$f"
        echo "   CRLF -> LF : ${f#"${PROFILE_DIR}/"}"
    fi
done < <(find "${PROFILE_DIR}" -type f -print0)

# --- 3. Liens symboliques (units systemd activees, localtime...) ---------------
log "Creation des liens symboliques (symlinks.list)"
AIROOTFS="${PROFILE_DIR}/airootfs"
while read -r link target _; do
    [[ -z "${link}" || "${link}" == \#* ]] && continue
    [[ -n "${target}" ]] || die "symlinks.list : cible manquante pour ${link}"
    mkdir -p -- "$(dirname -- "${AIROOTFS}${link}")"
    ln -sfn -- "${target}" "${AIROOTFS}${link}"
done < "${PROFILE_DIR}/symlinks.list"
rm -f -- "${PROFILE_DIR}/symlinks.list"

# --- 4. Injection des binaires --------------------------------------------------
if (( ! SKIP_RUST )); then
    log "Injection de ${BINARIES[*]} dans airootfs/usr/bin"
    {
        echo
        echo "# Ajoute par build/build-iso.sh"
    } >> "${PROFILE_DIR}/profiledef.sh"
    for b in "${BINARIES[@]}"; do
        install -Dm755 -- "${TARGET_DIR}/release/${b}" "${AIROOTFS}/usr/bin/${b}"
        echo "file_permissions+=([\"/usr/bin/${b}\"]=\"0:0:755\")" >> "${PROFILE_DIR}/profiledef.sh"
    done
fi
bash -n "${PROFILE_DIR}/profiledef.sh"

# --- 5. mkarchiso -------------------------------------------------------------
log "mkarchiso (work : ${WORK_DIR}, sortie : ${OUT_DIR})"
rm -rf -- "${WORK_DIR}"
mkdir -p -- "${OUT_DIR}"
MKARCHISO_LOG="/tmp/eloane-mkarchiso.log"
mkarchiso -v -w "${WORK_DIR}" -o "${OUT_DIR}" "${PROFILE_DIR}" 2>&1 | tee "${MKARCHISO_LOG}"
# mkarchiso ne s'arrete pas sur une erreur de mkinitcpio (hook manquant...) :
# l'initramfs serait incomplet et l'ISO ne demarrerait pas. On echoue ici.
if grep -E '^==> ERROR' "${MKARCHISO_LOG}"; then
    die "mkinitcpio a signale des erreurs (voir ci-dessus) : ISO probablement non bootable."
fi

(( KEEP_WORK )) || rm -rf -- "${WORK_DIR}" "${PROFILE_DIR}"

# Sur un hote Linux, rendre l'ISO au proprietaire du depot plutot qu'a root.
owner="$(stat -c '%u:%g' "${REPO}")"
[[ "${owner}" == "0:0" ]] || chown -R -- "${owner}" "${OUT_DIR}" || true

ISO="$(ls -1t "${OUT_DIR}"/eloane-*.iso 2>/dev/null | head -n1 || true)"
[[ -n "${ISO}" ]] || die "mkarchiso n'a produit aucune ISO dans ${OUT_DIR}"
ELAPSED=$(( $(date +%s) - START ))
log "ISO prete : ${ISO} ($(du -h -- "${ISO}" | cut -f1)) en $((ELAPSED / 60)) min $((ELAPSED % 60)) s"
