# install.ps1 — Install HADAL from a release archive on Windows.
#
# Requires:
#   $env:HADAL_DOWNLOAD_BASE  Base URL of release archives
#   $env:HADAL_VERSION        Version to install (default: 0.1.0)

param(
    [string]$Prefix = "$env:LOCALAPPDATA\hadal\bin"
)

$ErrorActionPreference = "Stop"

$version = if ($env:HADAL_VERSION) { $env:HADAL_VERSION } else { "0.1.0" }
$base = $env:HADAL_DOWNLOAD_BASE

if (-not $base) {
    Write-Error "HADAL_DOWNLOAD_BASE is not set."
    Write-Error "Set this to the base URL of your release archives."
    exit 1
}

# Detect architecture
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "AMD64") { "x86_64" } else { $env:PROCESSOR_ARCHITECTURE }
$target = "${arch}-pc-windows-msvc"
$binaryName = "hadal.exe"

$archiveName = "hadal-${version}-${target}.zip"
$checksumName = "hadal-${version}-${target}.sha256"
$archiveUrl = "$base/$archiveName"
$checksumUrl = "$base/$checksumName"

$env:TEMP = [System.IO.Path]::GetTempPath()
$tmpDir = Join-Path $env:TEMP ("hadal-install-" + (New-Guid).ToString())
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

try {
    Write-Host "Downloading $archiveName..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $archiveUrl -OutFile (Join-Path $tmpDir $archiveName) -UseBasicParsing

    Write-Host "Downloading $checksumName..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $checksumUrl -OutFile (Join-Path $tmpDir $checksumName) -UseBasicParsing

    # Verify checksum
    Write-Host "Verifying checksum..." -ForegroundColor Cyan
    $expectedHash = (Get-Content (Join-Path $tmpDir $checksumName) -Raw).Split(' ')[0]
    $actualHash = (Get-FileHash (Join-Path $tmpDir $archiveName) -Algorithm SHA256).Hash.ToLower()

    if ($expectedHash -ne $actualHash) {
        Write-Error "Checksum mismatch: expected $expectedHash, got $actualHash"
        exit 1
    }

    # Extract
    Write-Host "Extracting and installing..." -ForegroundColor Cyan
    $extractDir = Join-Path $tmpDir "extract"
    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead((Join-Path $tmpDir $archiveName))
    $zip.Entries | Where-Object { $_.Name -eq $binaryName } | ForEach-Object {
        $dest = Join-Path $extractDir $_.Name
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($_, $dest, $true)
    }
    $zip.Dispose()

    # Install
    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    Copy-Item (Join-Path $extractDir $binaryName) (Join-Path $Prefix $binaryName) -Force

    Write-Host "Installed: $Prefix\$binaryName" -ForegroundColor Green
    Write-Host ""
    Write-Host "To add to PATH, run:" -ForegroundColor Yellow
    Write-Host "  [Environment]::SetEnvironmentVariable('PATH', `$env:PATH + `';$Prefix', 'User')" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Run '$binaryName --version' to verify."
} finally {
    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
