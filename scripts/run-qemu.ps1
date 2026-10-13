# SPDX-License-Identifier: GPL-3.0-or-later
# Eloane - lance l'ISO la plus recente de out/ dans QEMU (Windows, UEFI/OVMF).
#
#   .\scripts\run-qemu.ps1
#   .\scripts\run-qemu.ps1 -Iso out\eloane-2026.10.13-x86_64.iso -Memory 6G -Cpus 2
#   .\scripts\run-qemu.ps1 -Accel tcg      # force l'emulation logicielle
#   .\scripts\run-qemu.ps1 -Logs           # boote l'entree 'Eloane - debug', port serie -> out\serial.log
#                                          # suivi dans une 2e fenetre, moniteur QEMU sur 127.0.0.1:4444
#
# Accelerateur : WHPX (Windows Hypervisor Platform, a activer dans
# "Fonctionnalites Windows") avec repli automatique sur TCG.
# Graphique : virtio-vga ; le rendu Vulkan se fait via lavapipe (vulkan-swrast)
# dans l'invite, donc lent mais suffisant pour verifier le boot et l'UI Steam.
[CmdletBinding()]
param(
    [string]$Iso,
    [string]$Memory = '8G',
    [int]$Cpus = 4,
    [ValidateSet('auto', 'whpx', 'tcg')]
    [string]$Accel = 'auto',
    [switch]$Logs
)

$ErrorActionPreference = 'Continue'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

# --- QEMU -------------------------------------------------------------------
$qemu = $null
$cmd = Get-Command 'qemu-system-x86_64' -ErrorAction SilentlyContinue
if ($cmd) {
    $qemu = $cmd.Source
} else {
    $candidates = @(
        (Join-Path $env:ProgramFiles 'qemu\qemu-system-x86_64.exe'),
        'C:\Program Files\qemu\qemu-system-x86_64.exe'
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $qemu = $c; break }
    }
}
if (-not $qemu) {
    Write-Host 'qemu-system-x86_64 introuvable (ni dans le PATH, ni dans C:\Program Files\qemu).'
    Write-Host 'Installation : winget install SoftwareFreedomConservancy.QEMU'
    exit 1
}
$qemuDir = Split-Path -Parent $qemu

# --- OVMF (UEFI) ----------------------------------------------------------------
$ovmfCode = $null
foreach ($c in @((Join-Path $qemuDir 'share\edk2-x86_64-code.fd'), (Join-Path $qemuDir 'edk2-x86_64-code.fd'))) {
    if (Test-Path $c) { $ovmfCode = $c; break }
}
if (-not $ovmfCode) {
    Write-Host "Firmware UEFI introuvable : $qemuDir\share\edk2-x86_64-code.fd"
    Write-Host 'Reinstaller QEMU (winget install SoftwareFreedomConservancy.QEMU).'
    exit 1
}
# Variables UEFI : copie modifiable du modele fourni par QEMU (sinon OVMF demarre sans NVRAM persistante).
$outDir = Join-Path $repo 'out'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$varsFile = Join-Path $outDir 'ovmf-vars.fd'
$varsTemplate = Join-Path (Split-Path -Parent $ovmfCode) 'edk2-i386-vars.fd'
if (-not (Test-Path $varsFile) -and (Test-Path $varsTemplate)) {
    Copy-Item $varsTemplate $varsFile
}

# --- ISO ----------------------------------------------------------------------
if (-not $Iso) {
    $latest = Get-ChildItem -Path $outDir -Filter 'eloane-*.iso' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $latest) {
        Write-Host "Aucune ISO eloane-*.iso dans $outDir. Construire d'abord : .\build\build.ps1"
        exit 1
    }
    $Iso = $latest.FullName
}
if (-not (Test-Path $Iso)) { Write-Host "ISO introuvable : $Iso"; exit 1 }
Write-Host "ISO   : $Iso"
Write-Host "QEMU  : $qemu"
Write-Host "OVMF  : $ovmfCode"

# --- Accelerateur ---------------------------------------------------------------
# QEMU essaie les -accel dans l'ordre : whpx puis tcg si WHPX est indisponible.
switch ($Accel) {
    'whpx' { $accelArgs = @('-accel', 'whpx,kernel-irqchip=off') }
    'tcg'  { $accelArgs = @('-accel', 'tcg,thread=multi') }
    default { $accelArgs = @('-accel', 'whpx,kernel-irqchip=off', '-accel', 'tcg,thread=multi') }
}

$qemuArgs = @(
    '-name', 'eloane',
    '-machine', 'q35',
    '-cpu', 'max'
) + $accelArgs + @(
    '-m', $Memory,
    '-smp', "$Cpus",
    '-drive', "if=pflash,format=raw,unit=0,readonly=on,file=$ovmfCode"
)
if (Test-Path $varsFile) {
    $qemuArgs += @('-drive', "if=pflash,format=raw,unit=1,file=$varsFile")
}
$qemuArgs += @(
    '-cdrom', $Iso,
    '-boot', 'd',
    '-device', 'virtio-vga',
    '-device', 'qemu-xhci',
    '-device', 'usb-tablet',
    '-device', 'usb-kbd',
    '-nic', 'user,model=virtio-net-pci',
    '-audiodev', 'dsound,id=snd0',
    '-device', 'intel-hda',
    '-device', 'hda-duplex,audiodev=snd0'
)

if ($Logs) {
    $serialLog = Join-Path $outDir 'serial.log'
    Set-Content -Path $serialLog -Value '' -Encoding ascii
    $qemuArgs += @('-serial', "file:$serialLog")
    # QEMU monitor on localhost, for scripted debugging (sendkey, screendump, info network).
    $qemuArgs += @('-monitor', 'tcp:127.0.0.1:4444,server,nowait')
    # Second window following the serial output (kernel, systemd, journal).
    $tail = "`$host.UI.RawUI.WindowTitle = 'Eloane - logs'; Get-Content -LiteralPath '$serialLog' -Wait"
    # PS 5.1 Start-Process does not quote array items: pass a single pre-quoted string.
    Start-Process powershell.exe -ArgumentList "-NoExit -NoProfile -Command `"$tail`""
    # Pick the "Eloane - debug" boot entry (3rd) automatically once the
    # systemd-boot menu shows up on the serial port.
    $null = Start-Job -ArgumentList $serialLog -ScriptBlock {
        param($log)
        for ($i = 0; $i -lt 240; $i++) {
            Start-Sleep -Milliseconds 250
            $txt = Get-Content -LiteralPath $log -Raw -ErrorAction SilentlyContinue
            if ($txt -and $txt.Contains('Boot in')) { break }
        }
        try {
            $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 4444)
            $w = New-Object System.IO.StreamWriter($c.GetStream())
            $w.AutoFlush = $true
            foreach ($k in @('down', 'down', 'ret')) { $w.WriteLine("sendkey $k"); Start-Sleep -Milliseconds 200 }
            $c.Close()
        } catch { }
    }
    Write-Host "Logs  : $serialLog (entree de boot 'Eloane - debug' selectionnee automatiquement)"
}

Write-Host "==> $qemu $($qemuArgs -join ' ')"
& $qemu @qemuArgs
exit $LASTEXITCODE
