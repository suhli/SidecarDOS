#Requires -RunAsAdministrator
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Package, [Parameter(Mandatory)][string]$Devcon)
$ErrorActionPreference = 'Stop'
$packagePath = (Resolve-Path -LiteralPath $Package).Path
$inf = Join-Path $packagePath 'SidecarDOS.inf'
foreach ($file in @($inf, (Join-Path $packagePath 'SidecarDOS.dll'), (Join-Path $packagePath 'SidecarDOS.cat'))) {
    if (!(Test-Path -LiteralPath $file)) { throw "Missing signed driver package file: $file" }
}
$tool = (Resolve-Path -LiteralPath $Devcon).Path
$existing = Get-PnpDevice -Class Display -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -eq 'SidecarDOS Virtual Display' }
if ($existing) { & pnputil.exe /add-driver $inf /install } else { & $tool install $inf 'Root\SidecarDOS' }
if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 1 -and $LASTEXITCODE -ne 3010) { throw "Driver installation failed: $LASTEXITCODE" }
Write-Host 'Driver package installed. Start SidecarDOS Host in your normal interactive user session.'
