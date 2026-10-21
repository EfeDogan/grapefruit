param(
    [string]$DestinationRoot = (Join-Path $PSScriptRoot "../apps/desktop/src-tauri/windows-tools")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ffmpegUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-21-13-06/ffmpeg-n8.1.3-20-gc8de997cb1-win64-gpl-8.1.zip"
$ffmpegSha256 = "3c018ceab05450a5ca7b628d102eee676ef6124580fab0c0bc033570d70523d5"
$popplerUrl = "https://github.com/oschwartz10612/poppler-windows/releases/download/v26.09.0-0/Release-26.09.0-0.zip"
$popplerSha256 = "7a6f256a0ddf7536182246a5733331bf4677cbcc34f4663774947ad34556c8d0"

function Get-VerifiedArchive {
    param(
        [string]$Url,
        [string]$ExpectedSha256,
        [string]$OutputPath
    )

    Invoke-WebRequest -Uri $Url -OutFile $OutputPath
    $actual = (Get-FileHash -Path $OutputPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $ExpectedSha256) {
        throw "SHA-256 mismatch for $Url. Expected $ExpectedSha256, got $actual."
    }
}

$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("file-formatter-tools-" + [guid]::NewGuid())
$ffmpegArchive = Join-Path $temporaryRoot "ffmpeg.zip"
$popplerArchive = Join-Path $temporaryRoot "poppler.zip"
$ffmpegExtract = Join-Path $temporaryRoot "ffmpeg"
$popplerExtract = Join-Path $temporaryRoot "poppler"

try {
    New-Item -ItemType Directory -Path $temporaryRoot -Force | Out-Null
    Get-VerifiedArchive -Url $ffmpegUrl -ExpectedSha256 $ffmpegSha256 -OutputPath $ffmpegArchive
    Get-VerifiedArchive -Url $popplerUrl -ExpectedSha256 $popplerSha256 -OutputPath $popplerArchive
    Expand-Archive -Path $ffmpegArchive -DestinationPath $ffmpegExtract
    Expand-Archive -Path $popplerArchive -DestinationPath $popplerExtract

    $ffmpegExecutable = Get-ChildItem -Path $ffmpegExtract -Filter "ffmpeg.exe" -File -Recurse | Select-Object -First 1
    if ($null -eq $ffmpegExecutable) {
        throw "ffmpeg.exe was not found in the pinned FFmpeg archive."
    }

    $pdftoppmExecutable = Get-ChildItem -Path $popplerExtract -Filter "pdftoppm.exe" -File -Recurse | Select-Object -First 1
    if ($null -eq $pdftoppmExecutable -or $pdftoppmExecutable.Directory.Name -ne "bin") {
        throw "Poppler Library/bin directory was not found in the pinned archive."
    }
    $popplerLibrary = $pdftoppmExecutable.Directory.Parent.FullName

    $ffmpegDestination = Join-Path $DestinationRoot "ffmpeg"
    $popplerDestination = Join-Path $DestinationRoot "poppler"
    Remove-Item -Path $ffmpegDestination -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Path $popplerDestination -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $ffmpegDestination -Force | Out-Null
    New-Item -ItemType Directory -Path $popplerDestination -Force | Out-Null

    Copy-Item -Path $ffmpegExecutable.FullName -Destination (Join-Path $ffmpegDestination "ffmpeg.exe")
    Get-ChildItem -Path $ffmpegExtract -File -Recurse |
        Where-Object { $_.Name -match "^(COPYING|LICENSE|README)(\..+)?$" } |
        ForEach-Object {
            Copy-Item -Path $_.FullName -Destination (Join-Path $ffmpegDestination $_.Name) -Force
        }
    Copy-Item -Path (Join-Path $popplerLibrary "*") -Destination $popplerDestination -Recurse -Force

    Write-Host "Prepared pinned Windows tools in $DestinationRoot"
}
finally {
    Remove-Item -Path $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
