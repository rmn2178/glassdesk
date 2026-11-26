# Requires PowerShell 7 and WinGet. Does not install or execute Nosih.
[CmdletBinding()]
param(
    [string]$Tag = '',
    [string]$OutputRoot = (Join-Path $PSScriptRoot '../packaging/winget')
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = 'crafter-station/Nosih'
$id = 'CrafterStation.Nosih'
$versionPattern = '(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)'
if ($Tag -and $Tag -cnotmatch "\Av?$versionPattern\z") {
    throw 'Tag must be a stable version in vX.Y.Z format.'
}
$headers = @{ Accept = 'application/vnd.github+json'; 'User-Agent' = 'Nosih-WinGet' }
if ($env:GH_TOKEN) { $headers.Authorization = "Bearer $env:GH_TOKEN" }
$endpoint = if ($Tag) { "tags/v$($Tag.TrimStart('v'))" } else { 'latest' }
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/$endpoint" -Headers $headers -TimeoutSec 60
if ($release.draft -or $release.prerelease -or $release.tag_name -cnotmatch "\Av$versionPattern\z") {
    throw 'Only published stable releases with a vX.Y.Z tag may be submitted.'
}
$version = $release.tag_name.Substring(1)
$filename = "Nosih_${version}_x64-setup.exe"
$url = "https://github.com/$repo/releases/download/v$version/$filename"
$assets = @($release.assets | Where-Object name -CEQ $filename)
if ($assets.Count -ne 1 -or $assets[0].browser_download_url -cne $url) {
    throw "Release v$version must contain exactly one versioned installer: $filename"
}
if (-not (Get-Command winget -ErrorAction SilentlyContinue)) { throw 'WinGet is required to validate manifests.' }

$download = Join-Path ([IO.Path]::GetTempPath()) "nosih-winget-$([guid]::NewGuid()).exe"
try {
    # Use the public, final release asset, after any Authenticode signing.
    Invoke-WebRequest $url -OutFile $download -TimeoutSec 120
    $file = Get-Item -LiteralPath $download
    $sha256 = (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash
    if ($file.Length -ne $assets[0].size) { throw 'The downloaded installer size differs from the release asset.' }
    if ($assets[0].digest -and $assets[0].digest -ine "sha256:$sha256") {
        throw 'The downloaded installer SHA-256 differs from the GitHub release digest.'
    }
    if ($file.VersionInfo.ProductVersion -ne $version -or $file.VersionInfo.ProductName -ne 'Nosih') {
        throw 'The installer product/version metadata differs from the release tag.'
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $download
    if ($signature.Status -notin @('Valid', 'NotSigned') -or
        $signature.SignerCertificate.Thumbprint -eq '3A93532146A10D3B1FA8DEAF4192AD4A232FFCAC') {
        throw 'The installer has an invalid or test Authenticode signature; publish a production installer first.'
    }
    $directory = Join-Path ([IO.Path]::GetFullPath($OutputRoot, $PWD.Path)) "manifests/c/CrafterStation/Nosih/$version"
    New-Item -ItemType Directory -Force $directory | Out-Null
    $values = @{
        VERSION = $version
        INSTALLER_URL = $url
        INSTALLER_SHA256 = $sha256
        RELEASE_DATE = ([DateTimeOffset]$release.published_at).UtcDateTime.ToString('yyyy-MM-dd')
    }
    $templates = Join-Path $PSScriptRoot '../packaging/winget/templates'
    foreach ($name in @("$id.yaml", "$id.installer.yaml", "$id.locale.en-US.yaml", "$id.locale.es-MX.yaml")) {
        $content = Get-Content -LiteralPath (Join-Path $templates $name) -Raw
        foreach ($key in $values.Keys) { $content = $content.Replace("{{$key}}", $values[$key]) }
        if ($content -match '\{\{[A-Z_]+\}\}') { throw "Unresolved placeholder in $name" }
        [IO.File]::WriteAllText((Join-Path $directory $name), $content.Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))
    }
    # Native output must not contaminate this script's structured return value.
    & winget validate --manifest $directory --disable-interactivity | Out-Host
    if ($LASTEXITCODE) { throw "WinGet rejected the generated manifest (exit $LASTEXITCODE)." }
    [pscustomobject]@{ Version = $version; Directory = $directory; Sha256 = $sha256; InstallerUrl = $url }
} finally {
    Remove-Item -LiteralPath $download -Force -ErrorAction SilentlyContinue
}
