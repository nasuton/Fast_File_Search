#Requires -Version 7.0
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Packaging requires Windows x86_64 and the MSVC toolchain.' }

$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$target = 'x86_64-pc-windows-msvc'
$originalTargetDir = $env:CARGO_TARGET_DIR
$originalRustflags = $env:RUSTFLAGS
$originalEncodedFlags = $env:CARGO_ENCODED_RUSTFLAGS
$originalTargetFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS

function Invoke-Cargo {
    param([string[]]$CargoArguments)
    & cargo @CargoArguments
    if ($LASTEXITCODE -ne 0) { throw "cargo $($CargoArguments -join ' ') failed (exit $LASTEXITCODE)" }
}

Push-Location $projectRoot
try {
    $about = Join-Path $projectRoot 'target/tools/bin/cargo-about.exe'
    if (-not (Test-Path -LiteralPath $about)) {
        $command = Get-Command cargo-about -ErrorAction SilentlyContinue
        if ($null -eq $command) {
            throw 'Install the license generator: cargo install cargo-about --version 0.9.1 --features cli --locked --root target/tools'
        }
        $about = $command.Source
    }
    $aboutVersion = & $about --version
    if ($LASTEXITCODE -ne 0 -or $aboutVersion -notmatch '\b0\.9\.1\b') {
        throw 'The release process requires cargo-about 0.9.1.'
    }

    $metadataJson = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read Cargo metadata.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $package = $metadata.packages | Where-Object name -EQ 'Fast_File_Search'
    $version = $package.version
    if ($version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') { throw 'Unexpected package version.' }

    # Isolate distributable builds from IDE output and user-provided compiler flags.
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target/distribution'
    $env:RUSTFLAGS = $null
    $env:CARGO_ENCODED_RUSTFLAGS = $null
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
    Invoke-Cargo -CargoArguments @('test', '--release', '--locked', '--target', $target, '--all-targets')
    Invoke-Cargo -CargoArguments @('build', '--release', '--locked', '--target', $target, '--bin', 'Fast_File_Search')

    $archiveName = "Fast_File_Search-v$version-windows-x86_64"
    $staging = Join-Path $projectRoot ("target/package-staging/" + [Guid]::NewGuid().ToString('N'))
    $packageDir = Join-Path $staging $archiveName
    $dist = Join-Path $projectRoot 'dist'
    New-Item -ItemType Directory -Path $packageDir, $dist -Force | Out-Null
    $binary = Join-Path $env:CARGO_TARGET_DIR "$target/release/Fast_File_Search.exe"
    Copy-Item -LiteralPath $binary -Destination $packageDir
    Copy-Item -LiteralPath LICENSE, CHANGELOG.md -Destination $packageDir
    Copy-Item -LiteralPath docs/USAGE.md -Destination (Join-Path $packageDir 'USAGE.md')
    Copy-Item -LiteralPath docs/DISTRIBUTION.md -Destination (Join-Path $packageDir 'README.md')

    # Do not use --offline: some crates omit notices from their published tarball.
    $licenses = Join-Path $packageDir 'THIRD_PARTY_LICENSES.html'
    & $about generate --locked --fail about.hbs --output-file $licenses
    if ($LASTEXITCODE -ne 0) { throw 'License report generation failed; no archive was created.' }
    if ((Get-Item -LiteralPath $licenses).Length -lt 1000) { throw 'License report is unexpectedly empty.' }

    $rustVersion = & rustc --version
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read rustc version.' }
    @(
        "Fast File Search $version"
        "Target: $target"
        "Compiler: $rustVersion"
        'CRT: static'
        'Dependencies: Cargo.lock (locked build)'
    ) | Set-Content -LiteralPath (Join-Path $packageDir 'BUILD-INFO.txt') -Encoding utf8NoBOM

    $archive = Join-Path $dist "$archiveName.zip"
    Compress-Archive -LiteralPath $packageDir -DestinationPath $archive -CompressionLevel Optimal -Force
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $archiveName.zip" | Set-Content -LiteralPath "$archive.sha256" -Encoding ascii
    Write-Host "Package: $archive"
    Write-Host "SHA-256: $hash"
    # Staging remains under ignored target/ for inspection; no recursive deletion.
}
finally {
    $env:CARGO_TARGET_DIR = $originalTargetDir
    $env:RUSTFLAGS = $originalRustflags
    $env:CARGO_ENCODED_RUSTFLAGS = $originalEncodedFlags
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $originalTargetFlags
    Pop-Location
}
