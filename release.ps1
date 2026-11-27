<#
.SYNOPSIS
Builds, signs and publishes the version from Cargo.toml as a GitHub pre-release.
Pre-releases are invisible to the updater of the old .NET app.
.NOTES
Once: cargo install rsign2
      rsign generate -p "$HOME\.rsign\pulsemqtt.pub" -s "$HOME\.rsign\pulsemqtt.key"
      Copy-Item "$HOME\.rsign\pulsemqtt.pub" update.pub
Keep the secret key and its password safe and out of the repository: whoever has them can
ship programs that run with admin rights on every installation.
#>
param([string]$Key = "$HOME\.rsign\pulsemqtt.key")
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Invoke-Step([string]$what, [scriptblock]$command) {
    & $command
    if ($LASTEXITCODE) { throw "$what failed" }
}

if (-not (Get-Content update.pub -Raw)) { throw 'update.pub is empty, see the notes in this script.' }
if (git status --porcelain) { throw 'Commit everything first.' }
Invoke-Step 'git fetch' { git fetch --quiet origin main }
$commit = git rev-parse HEAD
if ($commit -ne (git rev-parse origin/main)) { throw 'HEAD must be the pushed state of main.' }

$version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
Invoke-Step 'tests' { cargo test --locked }
Invoke-Step 'build' { cargo build --release --locked }

$program = 'target\release\pulsemqtt.exe'
$signature = "$program.minisig"
Invoke-Step 'signing' { rsign sign $program -s $Key -x $signature -t "version:$version" }
# Catches a key that does not belong to the public key built into the app.
Invoke-Step 'verification' { rsign verify $program -p update.pub -x $signature }

Invoke-Step 'release' {
    gh release create "v$version" $program $signature --repo kurim/pulse-mqtt --target $commit `
        --prerelease --title "PulseMQTT $version (Rust)" --generate-notes
}
