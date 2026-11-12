# Prepares a windows-11-arm runner to build the Rust workspace with the embedded llama.cpp
# (CMake + bindgen). Used by .github/workflows/release.yml and release-check.yml.
#
#   setup-windows-arm64.ps1 -Step rust    before dtolnay/rust-toolchain: rustup and a native libclang
#   setup-windows-arm64.ps1 -Step tools   after ilammy/msvc-dev-cmd (arch: arm64): CMake and Ninja
#
# Nothing here has been run on a real windows-11-arm runner yet: every step checks first what the
# image already has and only installs what is missing, so it is a no-op where the image is complete.
param([Parameter(Mandatory = $true)][ValidateSet('rust', 'tools')][string]$Step)
$ErrorActionPreference = 'Stop'

function Add-ToPath([string]$dir) {
  if ($env:GITHUB_PATH) { Add-Content -Path $env:GITHUB_PATH -Value $dir }
  $env:PATH = "$dir;$env:PATH"
}

# The PE machine type of a DLL: 0xAA64 is ARM64. bindgen loads libclang.dll into the arm64 build
# script process, so an x64 libclang.dll (the default of most LLVM installers) cannot be used.
function Get-PEMachine([string]$path) {
  $fs = [IO.File]::OpenRead($path)
  try {
    $br = New-Object IO.BinaryReader($fs)
    $fs.Position = 0x3C
    $pe = $br.ReadInt32()
    $fs.Position = $pe + 4
    return $br.ReadUInt16()
  } finally { $fs.Dispose() }
}

if ($Step -eq 'rust') {
  if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Host 'rustup not found: installing the arm64 rustup-init'
    $exe = Join-Path $env:RUNNER_TEMP 'rustup-init.exe'
    & curl.exe --fail --silent --show-error --location --retry 3 --connect-timeout 30 -o $exe 'https://win.rustup.rs/aarch64'
    if ($LASTEXITCODE -ne 0) { throw 'could not download rustup-init for aarch64' }
    & $exe -y --default-toolchain none --profile minimal
    if ($LASTEXITCODE -ne 0) { throw "rustup-init exited with $LASTEXITCODE" }
    Add-ToPath (Join-Path $env:USERPROFILE '.cargo\bin')
  }

  $candidates = @()
  if ($env:LIBCLANG_PATH) { $candidates += (Join-Path $env:LIBCLANG_PATH 'libclang.dll') }
  $candidates += 'C:\Program Files\LLVM\bin\libclang.dll', 'C:\LLVM-woa64\bin\libclang.dll'
  $native = $candidates | Where-Object { (Test-Path $_) -and ((Get-PEMachine $_) -eq 0xAA64) } | Select-Object -First 1
  if (-not $native) {
    # Official arm64-native LLVM installer (the "woa64" build), pinned.
    $version = '19.1.7'
    $installer = Join-Path $env:RUNNER_TEMP "LLVM-$version-woa64.exe"
    $url = "https://github.com/llvm/llvm-project/releases/download/llvmorg-$version/LLVM-$version-woa64.exe"
    Write-Host "No arm64 libclang.dll found: installing LLVM $version (woa64) from $url"
    & curl.exe --fail --silent --show-error --location --retry 3 --connect-timeout 30 --max-time 900 -o $installer $url
    if ($LASTEXITCODE -ne 0) { throw 'could not download the LLVM woa64 installer' }
    $bytes = [IO.File]::ReadAllBytes($installer)
    if (-not ($bytes.Length -gt 10MB -and $bytes[0] -eq 0x4D -and $bytes[1] -eq 0x5A)) { throw 'the LLVM download is not a Windows executable' }
    # NSIS installer: /D must be the last argument and unquoted.
    $p = Start-Process -FilePath $installer -ArgumentList @('/S', '/D=C:\LLVM-woa64') -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "the LLVM installer exited with $($p.ExitCode)" }
    $native = 'C:\LLVM-woa64\bin\libclang.dll'
    if (-not (Test-Path $native)) { throw "libclang.dll not found at $native after the install" }
    if ((Get-PEMachine $native) -ne 0xAA64) { throw 'the installed libclang.dll is not arm64' }
  }
  $dir = Split-Path $native -Parent
  Write-Host "libclang (arm64): $dir"
  if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value "LIBCLANG_PATH=$dir" }
}

if ($Step -eq 'tools') {
  $missing = @('cmake', 'ninja') | Where-Object { -not (Get-Command $_ -ErrorAction SilentlyContinue) }
  if ($missing) {
    Write-Host "Missing from the image: $($missing -join ', '): installing the pip wheels"
    & python -m pip install --user --disable-pip-version-check $missing
    if ($LASTEXITCODE -ne 0) { throw "pip could not install: $($missing -join ', ')" }
    $scripts = (& python -c "import sysconfig; print(sysconfig.get_path('scripts', 'nt_user'))").Trim()
    Add-ToPath $scripts
  }
  & cmake --version
  & ninja --version
  # llama.cpp's nested CMake projects want Ninja (see the Vulkan notes in docs/release/RELEASE.md).
  if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value 'CMAKE_GENERATOR=Ninja' }
}
