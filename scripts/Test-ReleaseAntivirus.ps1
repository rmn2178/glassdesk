# Offline regression tests: no real scan, malware, network or Defender changes.
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ("nosih-antivirus-test-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
$fixture = Join-Path $root 'fixture.exe'
$reportDir = Join-Path $root 'report'
Set-Content -LiteralPath $fixture -Value 'test-owned inert bytes'
$verifier = Join-Path $PSScriptRoot 'Assert-ReleaseAntivirus.ps1'
. $verifier -Path $fixture -ReportDirectory $reportDir
$state = @{ enabled = $true; age = 0; scan = 'clean'; calls = 0; download = 'clean'; downloadCalls = 0 }
function Get-MpComputerStatus {
    [pscustomobject]@{ AMServiceEnabled = $state.enabled; AntivirusEnabled = $state.enabled
        AntivirusSignatureAge = $state.age; AMEngineVersion = 'test-engine'; AntivirusSignatureVersion = 'test-definitions' }
}
function Get-ReleaseDefenderScanner { 'test-only scanner' }
function Invoke-ReleaseDefenderScan([string]$Scanner, [string]$File) {
    if ($Scanner -ne 'test-only scanner') { throw 'A real scanner must never run in offline tests.' }
    $state.calls++
    switch ($state.scan) {
        'detected' { throw 'Fixture threat detected.' }
        'error' { throw 'Fixture scanner could not finish.' }
        'changed' { Add-Content -LiteralPath $File -Value 'changed bytes' }
        'removed' { Remove-Item -LiteralPath $File }
    }
}
function Read-Report { Get-Content -LiteralPath (Join-Path $reportDir 'antivirus-report.json') -Raw | ConvertFrom-Json }
function Invoke-ReleaseAttachmentCheck([string]$File, [uri]$Source) {
    $state.downloadCalls++
    if ($Source.Scheme -ne 'https') { throw 'Unexpected source in fixture.' }
    switch ($state.download) {
        'rejected' { throw 'Fixture download rejected by Windows Attachment Services.' }
        'unavailable' { throw 'Fixture download check unavailable.' }
        'changed' { Add-Content -LiteralPath $File -Value 'changed by trust service' }
        'removed' { Remove-Item -LiteralPath $File }
    }
}
function Assert-Rejected([string]$Name, [scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (-not $rejected -or (Read-Report).passed) { throw "Failed to reject: $Name" }
    Write-Host "PASS: $Name"
}
try {
    # Compile and verify the SDK vtable without creating a COM object, scanning
    # a file or invoking Save. Its slot order is part of the native ABI.
    Add-Type -Path (Join-Path $PSScriptRoot 'ReleaseAttachmentCheck.cs') -ErrorAction Stop
    $attachmentType = [NosihRelease.AttachmentCheck].Assembly.GetType('NosihRelease.IAttachmentExecute')
    $slots = @($attachmentType.GetMethods() | Sort-Object MetadataToken | ForEach-Object Name)
    $expected = @('SetClientTitle', 'SetClientGuid', 'SetLocalPath', 'SetFileName', 'SetSource',
        'SetReferrer', 'CheckPolicy', 'Prompt', 'Save')
    if (($slots -join ',') -ne ($expected -join ',') -or
        $attachmentType.GUID.ToString() -ne '73db1241-1e85-4581-8e4f-a81e1d0f8c57' -or
        $attachmentType.GetMethod('Save').GetParameters().Count -ne 0) {
        throw 'Attachment Services interface differs from the Windows SDK ABI.'
    }
    Write-Host 'PASS: Attachment Services bridge compiles with SDK slot order and no execution method'
    foreach ($file in @($verifier, $PSCommandPath)) {
        $tokens = $null; $errors = $null
        $null = [Management.Automation.Language.Parser]::ParseFile($file, [ref]$tokens, [ref]$errors)
        if ($errors) { throw ($errors | Out-String) }
    }
    Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir
    $report = Read-Report
    if (-not $report.passed -or $report.files[0].sha256 -ne (Get-FileHash -LiteralPath $fixture).Hash -or
        $report.signatureVersion -ne 'test-definitions' -or $state.calls -ne 1) { throw 'Clean report is incomplete.' }
    Write-Host 'PASS: completed scan records the exact bytes and definition version'
    Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir -DownloadPath $fixture `
        -DownloadSource 'https://example.com/fixture.exe'
    $report = Read-Report
    if (-not $report.passed -or -not $report.files[0].downloadChecked -or $state.downloadCalls -ne 1 -or
        $report.downloadSource -ne 'https://example.com/fixture.exe') { throw 'Download check evidence is incomplete.' }
    Write-Host 'PASS: the final installer passes both antivirus and downloaded-file checks'
    foreach ($mode in @('rejected', 'unavailable', 'changed', 'removed')) {
        $state.download = $mode
        Assert-Rejected "download $mode" {
            Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir -DownloadPath $fixture `
                -DownloadSource 'https://example.com/fixture.exe'
        }
        Set-Content -LiteralPath $fixture -Value 'test-owned inert bytes'
    }
    $state.download = 'clean'
    Assert-Rejected 'download path without source' {
        Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir -DownloadPath $fixture
    }
    Assert-Rejected 'non-HTTPS download source' {
        Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir -DownloadPath $fixture `
            -DownloadSource 'http://example.com/fixture.exe'
    }
    $outside = Join-Path $root 'not-scanned.txt'
    Set-Content -LiteralPath $outside -Value 'inert fixture outside the scan list'
    Assert-Rejected 'download path outside the scan list' {
        Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir -DownloadPath $outside `
            -DownloadSource 'https://example.com/fixture.exe'
    }
    foreach ($mode in @('detected', 'error', 'changed', 'removed')) {
        $state.scan = $mode
        Assert-Rejected $mode { Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir }
        Set-Content -LiteralPath $fixture -Value 'test-owned inert bytes'
    }
    $state.scan = 'clean'
    $state.enabled = $false
    $calls = $state.calls
    Assert-Rejected 'disabled antivirus cannot pass' { Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir }
    if ($state.calls -ne $calls) { throw 'A disabled engine was used.' }
    $state.enabled = $true
    $state.age = 2
    Assert-Rejected 'stale definitions cannot pass' { Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir }
    $state.age = $null
    Assert-Rejected 'unknown definitions cannot pass' { Assert-ReleaseAntivirus -Path $fixture -ReportDirectory $reportDir }
    $state.age = 0
    Assert-Rejected 'missing release file cannot pass' { Assert-ReleaseAntivirus -Path (Join-Path $root 'missing.exe') -ReportDirectory $reportDir }
    $workflow = Get-Content (Join-Path $PSScriptRoot '../.github/workflows/release.yml') -Raw
    $scan = $workflow.IndexOf('- name: Analyze the final release files with Microsoft Defender')
    $prepare = $workflow.IndexOf('- name: Prepare release files')
    $publish = $workflow.IndexOf('- name: Create draft release')
    if ($scan -lt 0 -or $scan -ge $prepare -or $prepare -ge $publish) { throw 'Scan must precede updater manifest and release creation.' }
    if ($workflow -notmatch '-DownloadPath "target/release/bundle/nsis/' -or
        $workflow -notmatch '-DownloadSource "https://github.com/') { throw 'Release must check the downloaded installer context.' }
    Write-Host 'PASS: rejected files cannot reach updater metadata or a draft release'
} finally {
    # Only this test's newly created absolute temp directory may be removed.
    $resolved = [IO.Path]::GetFullPath($root)
    $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $resolved.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -notlike 'nosih-antivirus-test-*') { throw 'Unsafe test cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
