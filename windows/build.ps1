[CmdletBinding()]
param([ValidateSet('Debug','Release')][string]$Configuration = 'Release', [switch]$Driver)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & node protocol/generate.mjs --check
    if ($LASTEXITCODE -ne 0) { throw 'Generated protocol files are stale' }
    & cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Host tests failed' }
    & cargo clippy --workspace --all-targets --locked -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Host lint failed' }
    if ($Configuration -eq 'Release') { & cargo build --release --locked } else { & cargo build --locked }
    if ($LASTEXITCODE -ne 0) { throw 'Host build failed' }
    if ($Driver) {
        & msbuild windows/driver/SidecarDOS.vcxproj /m "/p:Configuration=$Configuration" /p:Platform=x64
        if ($LASTEXITCODE -ne 0) { throw 'WDK driver build failed' }
    }
} finally { Pop-Location }
