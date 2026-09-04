[CmdletBinding()]
param([Parameter(Mandatory)][string]$WdkRoot)
$ErrorActionPreference='Stop'
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$wdk=(Resolve-Path -LiteralPath $WdkRoot).Path
$vswhere=Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -property installationPath
$devcmd=Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
$environment=& cmd.exe /d /s /c ('"'+$devcmd+'" -arch=x64 -host_arch=x64 >nul && set')
foreach($line in $environment){if($line -match '^([^=]+)=(.*)$'){[Environment]::SetEnvironmentVariable($matches[1],$matches[2],'Process')}}
$out=Join-Path $root 'build\driver\Release'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$include=@("/I$wdk\Include\wdf\umdf\2.25","/I$wdk\Include\10.0.26100.0\um\iddcx\1.2")
$defines=@('/DUMDF_VERSION_MAJOR=2','/DUMDF_VERSION_MINOR=25','/DUMDF_USING_NTSTATUS','/DUNICODE','/D_UNICODE','/DNOMINMAX','/DIDDCX_VERSION_MAJOR=1','/DIDDCX_VERSION_MINOR=2')
& cl.exe /nologo /std:c++17 /EHsc /W4 /WX /MD /O2 @defines @include /c (Join-Path $PSScriptRoot 'Driver.cpp') "/Fo$out\Driver.obj"
if($LASTEXITCODE -ne 0){throw 'Driver compilation failed'}
& link.exe /nologo /DLL /SUBSYSTEM:WINDOWS "/OUT:$out\SidecarDOS.dll" "$out\Driver.obj" "$wdk\Lib\wdf\umdf\x64\2.25\WdfDriverStubUm.lib" "$wdk\Lib\10.0.26100.0\um\x64\iddcx\1.2\IddCxStub.lib" d3d11.lib dxgi.lib kernel32.lib user32.lib advapi32.lib ntdll.lib
if($LASTEXITCODE -ne 0){throw 'Driver linking failed'}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'SidecarDOS.inf') -Destination $out
& "$wdk\tools\10.0.26100.0\x64\infverif.exe" /w "$out\SidecarDOS.inf"
if($LASTEXITCODE -ne 0){throw 'Driver INF verification failed'}
& "$wdk\bin\10.0.26100.0\x86\Inf2Cat.exe" "/driver:$out" /os:10_GE_X64 /uselocaltime
if($LASTEXITCODE -ne 0){throw 'Driver catalog generation failed'}
Write-Host "Unsigned package: $out"
