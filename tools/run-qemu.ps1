# Runs Aeon in a QEMU window on Windows, so you can see and try it before putting it on a USB stick.
#
#   powershell -ExecutionPolicy Bypass -File tools\run-qemu.ps1            run the latest build (target\...\aeon.efi)
#   ... -Build                                                              run `cargo build --release` first
#   ... -Release                                                            run the signed files from usb\ instead
#   ... -Disk                                                               attach a demo FAT16 disk (try: disks, dls, dcat hello.txt)
#   ... -Mem 128                                                            RAM in MB (default 512; Aeon boots in 64)
#   ... -Cfg "mode=1920x1080"                                               write an aeon.cfg (boot options) on the boot disk
#
# Click inside the window to type; Ctrl+Alt+G gives the mouse back. Serial log: target\qemu-serial.log
param(
    [switch]$Build,
    [switch]$Release,
    [switch]$Disk,
    [int]$Mem = 512,
    [string]$Cfg = ""
)
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$qemuDir = "C:\Program Files\qemu"
$qemu = Join-Path $qemuDir "qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) { throw "QEMU not found at $qemu" }
$code = Join-Path $qemuDir "share\edk2-x86_64-code.fd"
$varsSrc = Join-Path $qemuDir "share\edk2-i386-vars.fd"

if ($Build) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

# The boot disk: a folder QEMU shows as a FAT drive with the image where the firmware looks for it.
$esp = Join-Path $repo "target\qemu-esp"
if (Test-Path $esp) { Remove-Item $esp -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $esp "EFI\BOOT") | Out-Null
if ($Release) {
    Copy-Item (Join-Path $repo "usb\EFI\BOOT\BOOTX64.EFI") (Join-Path $esp "EFI\BOOT\BOOTX64.EFI")
} else {
    $efi = Join-Path $repo "target\x86_64-unknown-uefi\release\aeon.efi"
    if (-not (Test-Path $efi)) { throw "No build yet: run with -Build" }
    Copy-Item $efi (Join-Path $esp "EFI\BOOT\BOOTX64.EFI")
}
if ($Cfg) { Set-Content -Path (Join-Path $esp "EFI\BOOT\aeon.cfg") -Value $Cfg -Encoding ascii }

# Fresh firmware variables each run (so boot counting and saved seeds start clean).
$vars = Join-Path $repo "target\qemu-vars.fd"
Copy-Item $varsSrc $vars -Force

$serial = Join-Path $repo "target\qemu-serial.log"
$fwd = { param($p) $p -replace '\\', '/' }
$qemuArgs = @(
    "-machine", "q35", "-cpu", "max", "-m", "$Mem", "-vga", "std",
    "-display", "gtk,zoom-to-fit=on", "-name", "Aeon",
    "-drive", "if=pflash,format=raw,unit=0,readonly=on,file=$(& $fwd $code)",
    "-drive", "if=pflash,format=raw,unit=1,file=$(& $fwd $vars)",
    "-drive", "format=raw,file=fat:rw:$(& $fwd $esp)",
    "-serial", "file:$(& $fwd $serial)",
    "-no-reboot"
)
if ($Disk) {
    $img = Join-Path $repo "target\demo-disk.img"
    python (Join-Path $repo "tests\mkdisk.py") $img | Out-Null
    $qemuArgs += @("-drive", "file=$(& $fwd $img),if=none,id=d0,format=raw,readonly=on", "-device", "virtio-blk-pci,drive=d0,disable-legacy=on")
}
"Starting Aeon ($Mem MB$(if ($Disk) { ', with demo disk' })). Close the window to stop it."
# Start-Process joins the arguments with spaces, so any argument that contains one needs its own quotes.
$quoted = $qemuArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
$p = Start-Process -FilePath $qemu -ArgumentList $quoted -PassThru
"QEMU pid $($p.Id); serial log: $serial"
