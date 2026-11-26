# Offline regression checks. Only test-owned temporary files are written;
# network, signature inspection and the native validator are replaced below.
$ErrorActionPreference = 'Stop'
$prepare = Join-Path $PSScriptRoot 'New-WingetManifest.ps1'
$submit = Join-Path $PSScriptRoot 'Submit-WingetManifest.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) "nosih-winget-test-$([guid]::NewGuid())"
$bytes = [Text.Encoding]::UTF8.GetBytes('Nosih installer fixture')
$hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
$savedToken = $env:WINGET_CREATE_GITHUB_TOKEN
$savedSummary = $env:GITHUB_STEP_SUMMARY
$savedExitCode = $global:LASTEXITCODE
$state = @{}
$state.downloads = 0
$state.apiCalls = 0
$state.validationExit = 0
$state.productVersion = '0.1.16'
$state.signatureStatus = 'NotSigned'
$state.certificateThumbprint = ''
$state.catalog = $null
$state.openPrs = @()
$state.fixture = [pscustomobject]@{
    tag_name = 'v0.1.16'; draft = $false; prerelease = $false
    published_at = '2026-11-20T00:56:52Z'
    assets = @([pscustomobject]@{
        name = 'Nosih_0.1.16_x64-setup.exe'; size = $bytes.Length; digest = "sha256:$hash"
        browser_download_url = 'https://github.com/crafter-station/Nosih/releases/download/v0.1.16/Nosih_0.1.16_x64-setup.exe'
    })
}
function Invoke-RestMethod {
    param($Uri, $Headers, $TimeoutSec)
    $state.apiCalls++
    if ($Uri -like 'https://api.github.com/repos/crafter-station/Nosih/releases/*') { return $state.fixture }
    if ($Uri -like 'https://api.github.com/repos/microsoft/winget-pkgs/contents/*') { return $state.catalog }
    if ($Uri -like 'https://api.github.com/search/issues?*') { return @{ items = $state.openPrs } }
    throw "Unexpected test API call: $Uri"
}
function Invoke-WebRequest {
    param($Uri, $OutFile, $TimeoutSec)
    $state.downloads++
    [IO.File]::WriteAllBytes($OutFile, $bytes)
}
function Get-Item {
    param($LiteralPath)
    [pscustomobject]@{
        Length = $bytes.Length
        VersionInfo = @{ ProductVersion = $state.productVersion; ProductName = 'Nosih' }
    }
}
function Get-AuthenticodeSignature {
    param($LiteralPath)
    @{ Status = $state.signatureStatus; SignerCertificate = @{ Thumbprint = $state.certificateThumbprint } }
}
function winget {
    if ($args[0] -ne 'validate') { throw 'Only manifest validation is allowed in these tests.' }
    $global:LASTEXITCODE = $state.validationExit
}
# Capture the fixture object so a child script's scope cannot hide it.
foreach ($name in @('Invoke-RestMethod', 'Invoke-WebRequest', 'Get-Item', 'Get-AuthenticodeSignature', 'winget')) {
    $body = (Get-Command $name -CommandType Function).ScriptBlock
    Set-Item -LiteralPath "Function:$name" -Value ($body.GetNewClosure())
}
function Assert-Rejected([string]$Name, [scriptblock]$Action, [string]$Message) {
    $rejected = $false
    try { & $Action | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $Message) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Failed to reject: $Name" }
    Write-Host "PASS: $Name"
}
try {
    $env:WINGET_CREATE_GITHUB_TOKEN = ''
    $env:GITHUB_STEP_SUMMARY = ''
    foreach ($file in @($prepare, $submit)) {
        $tokens = $null; $errors = $null
        $null = [Management.Automation.Language.Parser]::ParseFile($file, [ref]$tokens, [ref]$errors)
        if ($errors) { throw ($errors | Out-String) }
    }
    $manifest = & $prepare -Tag v0.1.16 -OutputRoot $root
    if ($manifest.Version -ne '0.1.16' -or $manifest.Sha256 -ne $hash -or
        @(Get-ChildItem -LiteralPath $manifest.Directory -Filter '*.yaml').Count -ne 4) {
        throw 'The valid release did not produce exactly four manifests with the computed hash.'
    }
    Write-Host 'PASS: stable published release generates four manifests'
    $installerText = Get-Content -LiteralPath (Join-Path $manifest.Directory 'CrafterStation.Nosih.installer.yaml') -Raw
    if ($installerText -notmatch '(?m)^  Silent: /S\s*$' -or
        $installerText -notmatch '(?m)^  SilentWithProgress: /S\s*$' -or
        $installerText -match '(?m)^  SilentWithProgress: /P\s*$') {
        throw 'Both unattended installation modes must use /S; /P can open first-install Settings.'
    }
    if ($installerText -notmatch 'PackageIdentifier: Microsoft\.VCRedist\.2015\+\.x64' -or
        $installerText -notmatch 'PackageIdentifier: Microsoft\.EdgeWebView2Runtime') {
        throw 'The generated manifest must declare the dock CRT and settings WebView2 prerequisites.'
    }
    Write-Host 'PASS: unattended modes and clean-machine runtime dependencies are declared'
    Assert-Rejected 'tag injection' { & $prepare -Tag 'v0.1.16; whoami' -OutputRoot $root } 'Tag must be*'
    $state.fixture.prerelease = $true
    Assert-Rejected 'prerelease' { & $prepare -OutputRoot $root } 'Only published stable releases*'
    $state.fixture.prerelease = $false
    $state.fixture.assets[0].browser_download_url = 'https://example.invalid/Nosih-Setup.exe'
    Assert-Rejected 'unversioned or foreign installer URL' { & $prepare -OutputRoot $root } '*exactly one versioned installer*'
    $state.fixture.assets[0].browser_download_url = 'https://github.com/crafter-station/Nosih/releases/download/v0.1.16/Nosih_0.1.16_x64-setup.exe'
    $state.fixture.assets[0].digest = 'sha256:BAD'
    Assert-Rejected 'altered asset digest' { & $prepare -OutputRoot $root } '*SHA-256 differs*'
    $state.fixture.assets[0].digest = "sha256:$hash"
    $state.productVersion = '0.1.15'
    Assert-Rejected 'mismatched installer version' { & $prepare -OutputRoot $root } '*product/version metadata*'
    $state.productVersion = '0.1.16'
    $state.signatureStatus = 'HashMismatch'
    Assert-Rejected 'invalid Authenticode' { & $prepare -OutputRoot $root } '*invalid or test Authenticode*'
    $state.signatureStatus = 'Valid'
    $state.certificateThumbprint = '3A93532146A10D3B1FA8DEAF4192AD4A232FFCAC'
    Assert-Rejected 'trusted local test certificate' { & $prepare -OutputRoot $root } '*invalid or test Authenticode*'
    $state.certificateThumbprint = ''
    $state.signatureStatus = 'NotSigned'
    $state.validationExit = 1
    Assert-Rejected 'native validator failure' { & $prepare -OutputRoot $root } 'WinGet rejected*'
    $state.validationExit = 0
    $beforeSubmit = $state.downloads
    Assert-Rejected 'missing submission token' { & $submit -ManifestDirectory $manifest.Directory } 'WINGET_TOKEN is missing*'
    $env:WINGET_CREATE_GITHUB_TOKEN = 'github_pat_fixture_not_a_real_token'
    $beforeTokenCheck = $state.apiCalls
    Assert-Rejected 'fine-grained PAT rejected before API access or tool download' { & $submit -ManifestDirectory $manifest.Directory } 'WINGET_TOKEN is a fine-grained PAT*'
    if ($state.apiCalls -ne $beforeTokenCheck -or $state.downloads -ne $beforeSubmit) {
        throw 'Unsupported credentials reached a network or executable download boundary.'
    }
    $env:WINGET_CREATE_GITHUB_TOKEN = ''
    $state.catalog = @{ content = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("InstallerSha256: $hash`n")) }
    & $submit -ManifestDirectory $manifest.Directory
    Write-Host 'PASS: accepted version is not resubmitted'
    $state.catalog = @{ content = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('InstallerSha256: DIFFERENT')) }
    Assert-Rejected 'replacing a published hash' { & $submit -ManifestDirectory $manifest.Directory } '*different hash*'
    $state.catalog = $null
    $state.openPrs = @(@{ title = 'New package: CrafterStation.Nosih.Beta version 0.1.16'; html_url = 'https://example.invalid/wrong-package' })
    Assert-Rejected 'other package is not treated as our PR' { & $submit -ManifestDirectory $manifest.Directory } 'WINGET_TOKEN is missing*'
    $state.openPrs = @(@{ title = 'New version: CrafterStation.Nosih version 0.1.160'; html_url = 'https://example.invalid/wrong-version' })
    Assert-Rejected 'other version is not treated as our PR' { & $submit -ManifestDirectory $manifest.Directory } 'WINGET_TOKEN is missing*'
    $state.openPrs = @(@{ title = 'New package: CrafterStation.Nosih version 0.1.16'; html_url = 'https://example.invalid/test-pr' })
    & $submit -ManifestDirectory $manifest.Directory
    if ($state.downloads -ne $beforeSubmit) { throw 'A duplicate/missing-token case downloaded a submission executable.' }
    Write-Host 'PASS: open PR is not duplicated; submission never executes a tool without a token'
} finally {
    $env:WINGET_CREATE_GITHUB_TOKEN = $savedToken
    $env:GITHUB_STEP_SUMMARY = $savedSummary
    $global:LASTEXITCODE = $savedExitCode
    # Check the absolute target before recursively removing the test directory.
    $resolved = [IO.Path]::GetFullPath($root)
    if (-not $resolved.StartsWith([IO.Path]::GetTempPath(), [StringComparison]::OrdinalIgnoreCase) -or
        -not [IO.Path]::GetFileName($resolved).StartsWith('nosih-winget-test-')) { throw 'Unsafe test cleanup path' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
