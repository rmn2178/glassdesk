# Uses Microsoft's submission client for both a first package and updates.
# The PAT is read from WINGET_CREATE_GITHUB_TOKEN, never a command-line argument.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ManifestDirectory)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$id = 'CrafterStation.Nosih'
$installer = Get-Content -LiteralPath (Join-Path $ManifestDirectory "$id.installer.yaml") -Raw
$version = [regex]::Match($installer, "(?m)^PackageVersion: '?([0-9]+\.[0-9]+\.[0-9]+)'?\s*$").Groups[1].Value
$sha256 = [regex]::Match($installer, '(?m)^\s+InstallerSha256: ([A-Fa-f0-9]{64})\s*$').Groups[1].Value
if (-not $version -or -not $sha256 -or $installer -notmatch '(?m)^PackageIdentifier: CrafterStation\.Nosih\s*$') {
    throw 'Expected a generated, validated CrafterStation.Nosih installer manifest.'
}
# WinGetCreate does not support fine-grained PATs, including ones that can
# read all public repositories. They cannot perform this cross-repository flow.
if ($env:WINGET_CREATE_GITHUB_TOKEN -clike 'github_pat_*') {
    throw 'WINGET_TOKEN is a fine-grained PAT, which WinGetCreate does not support. Create a token under GitHub Settings > Developer settings > Personal access tokens > Tokens (classic), select public_repo, replace the WINGET_TOKEN Actions secret, and rerun this same published version with Submit enabled. See https://github.com/settings/tokens/new and FIRMA-DE-CODIGO.md.'
}
$headers = @{ Accept = 'application/vnd.github+json'; 'User-Agent' = 'Nosih-WinGet' }
$lookupToken = if ($env:WINGET_CREATE_GITHUB_TOKEN) { $env:WINGET_CREATE_GITHUB_TOKEN } else { $env:GH_TOKEN }
if ($lookupToken) { $headers.Authorization = "Bearer $lookupToken" }
function Get-GitHubJson([string]$Path) {
    try { Invoke-RestMethod "https://api.github.com/$Path" -Headers $headers -TimeoutSec 60 }
    catch {
        if ([int]$_.Exception.Response.StatusCode -eq 404) { return $null }
        throw
    }
}
function Write-Result([string]$Message) {
    Write-Host $Message
    if ($env:GITHUB_STEP_SUMMARY) { $Message >> $env:GITHUB_STEP_SUMMARY }
}

# Idempotent retries: do not replace a published hash or open duplicate PRs.
$existing = Get-GitHubJson "repos/microsoft/winget-pkgs/contents/manifests/c/CrafterStation/Nosih/$version/$id.installer.yaml"
if ($existing) {
    $published = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($existing.content))
    if ($published -notmatch "(?im)^\s*InstallerSha256: $sha256\s*$") {
        throw 'This version already exists in WinGet with a different hash. Publish a new version instead of replacing its installer.'
    }
    Write-Result "Already accepted in WinGet: $id $version. No PR was created."
    return
}
$query = [Uri]::EscapeDataString("repo:microsoft/winget-pkgs is:pr is:open in:title `"$id`" `"$version`"")
$prs = Get-GitHubJson "search/issues?q=$query&per_page=100"
$versionTitle = '(?<![\w.+-])' + [regex]::Escape($version) + '(?![\w.+-])'
$idTitle = '(?<![\w.-])' + [regex]::Escape($id) + '(?![\w.-])'
$open = $prs.items | Where-Object {
    $_.title -match $idTitle -and $_.title -match $versionTitle
} | Select-Object -First 1
if ($open) {
    Write-Result "Submission already awaiting review: $($open.html_url)"
    return
}
if (-not $env:WINGET_CREATE_GITHUB_TOKEN) {
    throw 'WINGET_TOKEN is missing. Add the repository Actions secret and rerun this workflow with Submit enabled; see FIRMA-DE-CODIGO.md. The validated artifact remains available.'
}

# Pin both version and digest; do not execute an unverified latest download.
$tool = Join-Path ([IO.Path]::GetTempPath()) "nosih-wingetcreate-$([guid]::NewGuid()).exe"
try {
    $package = Get-GitHubJson 'repos/microsoft/winget-pkgs/contents/manifests/c/CrafterStation/Nosih'
    $kind = if ($package) { 'New version' } else { 'New package' }
    Invoke-WebRequest 'https://github.com/microsoft/winget-create/releases/download/v1.28.1.0/wingetcreate.exe' -OutFile $tool -TimeoutSec 120
    if ((Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash -ne '63D980BA79E97042395BD6EB5992C2DB9E01E9E7A8A22958A95C288F27310413') {
        throw 'The Microsoft WinGetCreate download did not match its pinned SHA-256.'
    }
    & $tool submit --no-open --prtitle "${kind}: $id version $version" $ManifestDirectory
    if ($LASTEXITCODE) {
        throw "WinGetCreate submission failed (exit $LASTEXITCODE). Check the client output above. For token permission errors, use a classic PAT with public_repo, check expiry/SSO authorization, replace the WINGET_TOKEN Actions secret and rerun this same published version. The validated artifact remains available."
    }
    Write-Result "Submitted $id $version to microsoft/winget-pkgs. Microsoft review and catalog indexing are still required."
} finally {
    Remove-Item -LiteralPath $tool -Force -ErrorAction SilentlyContinue
}
