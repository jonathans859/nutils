# Build THIRD-PARTY-NOTICES.txt for the release zip: the copyright notices and
# license texts of everything built into nutils.exe, nutils_hook.dll and
# nutils-settings.exe.
#
#   1. Rust crates, collected by cargo-about (about.toml / about.hbs).
#   2. The C/C++ code those crates compile in, which cargo-about can't see:
#      Prism's native library and the libraries it bundles (licenses shipped in
#      its external/prism/LICENSES folder), and wxWidgets.
#   3. Code copied into this repo from other projects (licenses/).
#
# Needs cargo-about on PATH. Usage: .\scripts\notices.ps1 [-Out path]
param([string]$Out = "THIRD-PARTY-NOTICES.txt")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $rule = "-" * 78
    cargo about generate --workspace --all-features about.hbs -o $Out
    if ($LASTEXITCODE -ne 0) { throw "cargo about failed" }
    $text = [System.Collections.Generic.List[string]]::new()

    # Prism's native library: its folder sits two levels above the prism-sys crate.
    $meta = cargo metadata --format-version 1 --all-features | ConvertFrom-Json
    $prismSys = $meta.packages | Where-Object name -eq "prism-sys" | Select-Object -First 1
    if (-not $prismSys) { throw "prism-sys not found in cargo metadata" }
    $licenses = Join-Path (Split-Path -Parent $prismSys.manifest_path) "..\..\external\prism\LICENSES"
    if (-not (Test-Path $licenses)) { throw "Prism licenses not found at $licenses" }
    foreach ($dir in Get-ChildItem $licenses -Directory | Sort-Object Name) {
        foreach ($file in Get-ChildItem $dir.FullName -File | Sort-Object Name) {
            $text.Add($rule)
            $text.Add("$($dir.Name) ($($file.Name)), compiled into nutils.exe as part of Prism")
            $text.Add("Prism source: https://github.com/garo-pro/prism2rust (external/prism)")
            $text.Add("")
            $text.Add((Get-Content $file.FullName -Raw))
        }
    }

    # wxWidgets (compiled into nutils-settings.exe) and code copied from other projects.
    $vendored = [ordered]@{
        "wxWidgets.txt" = "wxWidgets, compiled into nutils-settings.exe - https://www.wxwidgets.org"
        "Fedra.txt"     = "Fedra, whose screen-reader live region is used in nutils-settings.exe - https://github.com/trypsynth/fedra"
    }
    foreach ($name in $vendored.Keys) {
        $text.Add($rule)
        $text.Add($vendored[$name])
        $text.Add("")
        $text.Add((Get-Content (Join-Path "licenses" $name) -Raw))
    }

    Add-Content -Path $Out -Value $text -Encoding utf8
}
finally {
    Pop-Location
}
