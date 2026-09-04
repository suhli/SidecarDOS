#Requires -RunAsAdministrator
[CmdletBinding()]
param([Parameter(Mandatory)][string]$HostExecutable, [ValidateRange(1025,65535)][int]$Port = 47736)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $HostExecutable).Path
foreach ($name in @('SidecarDOS-QUIC','SidecarDOS-mDNS')) { Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue | Remove-NetFirewallRule }
New-NetFirewallRule -Name 'SidecarDOS-QUIC' -DisplayName 'SidecarDOS QUIC (local private network)' -Direction Inbound -Action Allow -Program $exe -Protocol UDP -LocalPort $Port -Profile Private -RemoteAddress LocalSubnet | Out-Null
New-NetFirewallRule -Name 'SidecarDOS-mDNS' -DisplayName 'SidecarDOS discovery (local private network)' -Direction Inbound -Action Allow -Program $exe -Protocol UDP -LocalPort 5353 -Profile Private -RemoteAddress LocalSubnet | Out-Null
