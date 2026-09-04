[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$package=Join-Path $root 'build\package\SidecarDOS'
New-Item -ItemType Directory -Force -Path $package,(Join-Path $package 'driver'),(Join-Path $package 'installer') | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'target\release\sidecardos-host.exe') -Destination $package
foreach($name in @('SidecarDOS.dll','SidecarDOS.inf','sidecardos.cat')){
    Copy-Item -LiteralPath (Join-Path $root "build\driver\Release\$name") -Destination (Join-Path $package 'driver')
}
Get-ChildItem -LiteralPath (Join-Path $root 'windows\installer') -Filter '*.ps1' | Copy-Item -Destination (Join-Path $package 'installer')
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination $package
New-Item -ItemType Directory -Force -Path (Join-Path $package 'docs'),(Join-Path $package 'protocol') | Out-Null
Get-ChildItem -LiteralPath (Join-Path $root 'docs') -Filter '*.md' | Copy-Item -Destination (Join-Path $package 'docs')
Copy-Item -LiteralPath (Join-Path $root 'protocol\README.md') -Destination (Join-Path $package 'protocol')
Set-Content -LiteralPath (Join-Path $package 'UNSIGNED-DRIVER.txt') -Value 'The driver in this developer package is unsigned. Follow README signing instructions on a test machine before installing it. The Windows-to-iPad physical-device acceptance tests have not been completed.'
Set-Content -LiteralPath (Join-Path $package 'INSTALL.txt') -Value @'
SidecarDOS Windows developer package

The driver is unsigned and must be signed and trusted on a dedicated test machine before installation. This package does not change Windows signing policy. See README.md and docs/validation.md for requirements and what has been verified.

After signing, use an administrator PowerShell:
  ./installer/Install-Driver.ps1 -Package ./driver -Devcon <path-to-WDK-devcon.exe>
  ./installer/Configure-Firewall.ps1 -HostExecutable ./sidecardos-host.exe

Start ./sidecardos-host.exe as your ordinary logged-in user. The iPad client must be built and signed from the repository using Xcode. Optional current-user startup:
  ./installer/Set-Autostart.ps1 -HostExecutable ./sidecardos-host.exe

The README build commands refer to the source repository, not this binary package.
'@
$zip=Join-Path $root 'build\SidecarDOS-windows-x64.zip'
Compress-Archive -LiteralPath $package -DestinationPath $zip -Force
Write-Host $zip
