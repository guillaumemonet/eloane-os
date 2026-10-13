# SPDX-License-Identifier: GPL-3.0-or-later
# Eloane - construit l'ISO depuis un hote Windows (PowerShell 5.1+) via Docker.
#
#   .\build\build.ps1            # build complet (workspace Rust + ISO)
#   .\build\build.ps1 -SkipRust  # ISO sans eloane-session / eloane-ctl
#   .\build\build.ps1 -NoCache   # reconstruit l'image eloane-builder de zero
#
# Volumes nommes (persistants entre deux builds) :
#   eloane-pacman-cache -> /var/cache/pacman/pkg  (paquets Arch, ~1.5 Go)
#   eloane-cargo        -> /cargo                 (registre crates.io)
#   eloane-target       -> /build/target          (artefacts cargo)
# Pour tout purger : docker volume rm eloane-pacman-cache eloane-cargo eloane-target
[CmdletBinding()]
param(
    [switch]$SkipRust,
    [switch]$NoCache,
    [string]$Image = 'eloane-builder'
)

# Pas de 'Stop' : sous PS 5.1, le stderr redirige d'un exe natif (docker) leverait une exception.
$ErrorActionPreference = 'Continue'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$start = Get-Date

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Error 'docker introuvable dans le PATH. Installer Docker Desktop : winget install Docker.DockerDesktop'
    exit 1
}

# Docker Desktop peut mettre un moment a demarrer : quelques tentatives.
$ready = $false
for ($i = 1; $i -le 12; $i++) {
    docker info --format '{{.ServerVersion}}' *> $null
    if ($LASTEXITCODE -eq 0) { $ready = $true; break }
    Write-Host "Docker n'est pas pret (tentative $i/12), nouvelle tentative dans 5 s..."
    Start-Sleep -Seconds 5
}
if (-not $ready) {
    Write-Error 'Le daemon Docker ne repond pas (docker info). Demarrer Docker Desktop puis relancer.'
    exit 1
}

Write-Host "==> docker build -t $Image (contexte : build/)"
$buildArgs = @('build', '-t', $Image)
if ($NoCache) { $buildArgs += @('--no-cache', '--pull') }
$buildArgs += (Join-Path $repo 'build')
& docker @buildArgs
if ($LASTEXITCODE -ne 0) { Write-Error "docker build a echoue (code $LASTEXITCODE)"; exit $LASTEXITCODE }

$scriptArgs = @()
if ($SkipRust) { $scriptArgs += '--skip-rust' }

# --privileged : mkarchiso utilise unshare/mount (pacstrap) dans le conteneur.
$runArgs = @(
    'run', '--rm', '--privileged',
    '-v', "${repo}:/src",
    '-v', 'eloane-pacman-cache:/var/cache/pacman/pkg',
    '-v', 'eloane-cargo:/cargo',
    '-v', 'eloane-target:/build/target',
    $Image, 'bash', '/src/build/build-iso.sh'
) + $scriptArgs

Write-Host "==> docker $($runArgs -join ' ')"
& docker @runArgs
$code = $LASTEXITCODE
$elapsed = (Get-Date) - $start
if ($code -ne 0) {
    Write-Error ("Build ISO en echec (code {0}) apres {1:hh\:mm\:ss}" -f $code, $elapsed)
    exit $code
}

$iso = Get-ChildItem -Path (Join-Path $repo 'out') -Filter 'eloane-*.iso' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($iso) {
    Write-Host ("==> ISO : {0} ({1:N2} Go) en {2:hh\:mm\:ss}" -f $iso.FullName, ($iso.Length / 1GB), $elapsed)
} else {
    Write-Warning 'Build termine mais aucune ISO trouvee dans out/.'
}
