# Select a signing lane before building. Test signatures never reach Releases.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('push', 'workflow_dispatch')][string]$EventName,
    [Parameter(Mandatory)][string]$Version,
    [string]$RefName = '',
    [string]$Organization = '',
    [string]$Project = '',
    [string]$Policy = '',
    [string]$ProductionThumbprint = '',
    [bool]$TokenPresent = $false
)
$ErrorActionPreference = 'Stop'
if ($Version -cnotmatch '\A(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)\z') {
    throw 'A stable workspace version is required.'
}
if ($EventName -eq 'push' -and $RefName -cne "v$Version") {
    throw "Tag $RefName does not match the workspace version $Version"
}
if ($EventName -eq 'push' -and -not $Organization.Trim()) {
    return [pscustomobject]@{ Mode = 'Unsigned'; Policy = ''; Version = $Version }
}
if (-not $Organization.Trim() -or -not $Project.Trim() -or -not $TokenPresent) {
    throw 'Configure SIGNPATH_ORGANIZATION_ID, SIGNPATH_PROJECT_SLUG and the SIGNPATH_API_TOKEN secret before requesting signing.'
}
if ($EventName -eq 'workflow_dispatch') {
    return [pscustomobject]@{ Mode = 'Test'; Policy = 'test-signing'; Version = $Version }
}
if ($Policy -cne 'release-signing') {
    throw 'A public release requires SIGNPATH_SIGNING_POLICY_SLUG=release-signing. Use Actions > Release > Run workflow for isolated test-signing; it does not create a release.'
}
if ($ProductionThumbprint -cnotmatch '\A[0-9A-Fa-f]{40}\z' -or
    $ProductionThumbprint -eq '3A93532146A10D3B1FA8DEAF4192AD4A232FFCAC') {
    throw 'Set SIGNPATH_PRODUCTION_CERTIFICATE_THUMBPRINT to the production certificate thumbprint confirmed in SignPath.'
}
[pscustomobject]@{ Mode = 'Production'; Policy = $Policy; Version = $Version }
