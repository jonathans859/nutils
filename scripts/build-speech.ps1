# Build nutils.exe with spoken feedback routed through Prism (the `speech`
# feature): a single self-contained exe that talks to NVDA over its RPC
# endpoint, with no nvdaControllerClient64.dll to ship alongside.
#
#   .\scripts\build-speech.ps1            # all backends (needs the VS C++ ATL component)
#   .\scripts\build-speech.ps1 -NoAtl     # skip the four ATL backends (keeps NVDA)
#
# See docs/BUILDING.md for what -NoAtl gives up.
param([switch]$NoAtl)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$env:PRISM_STATIC = "1"
# Absolute: CMake resolves a relative toolchain path against its build dir.
$env:CMAKE_TOOLCHAIN_FILE = Join-Path $root "cmake\prism.cmake"
if ($NoAtl) { $env:NUTILS_PRISM_NO_ATL = "1" } else { Remove-Item Env:NUTILS_PRISM_NO_ATL -ErrorAction SilentlyContinue }

Write-Host "Building nutils with Prism speech ($(if ($NoAtl) { 'no ATL backends' } else { 'all backends' }))..."
cargo build --release -p nutils --features speech
