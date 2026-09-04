#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess)]
param()
$ErrorActionPreference = 'Stop'
$devices = Get-PnpDevice -Class Display -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -eq 'SidecarDOS Virtual Display' }
foreach ($device in $devices) {
    $hardware = (Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds').Data
    if ($hardware -notcontains 'Root\SidecarDOS') { continue }
    if ($PSCmdlet.ShouldProcess($device.InstanceId, 'Remove SidecarDOS virtual display device')) {
        & pnputil.exe /remove-device $device.InstanceId
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) { throw "Device removal failed: $LASTEXITCODE" }
    }
}
foreach ($name in @('SidecarDOS-QUIC','SidecarDOS-mDNS')) {
    if ($PSCmdlet.ShouldProcess($name, 'Remove firewall rule')) { Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue | Remove-NetFirewallRule }
}
