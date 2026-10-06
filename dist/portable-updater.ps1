param(
    [int]$ProcessId,
    [string]$ZipPath,
    [string]$AppDirectory,
    [string]$ExecutablePath,
    [string]$LogPath
)

$ErrorActionPreference = "Stop"
$stage = Join-Path ([IO.Path]::GetTempPath()) ("Dromaius-update-" + [guid]::NewGuid())
$backup = Join-Path $stage "backup"
$payload = Join-Path $stage "payload"
$success = $false

function Write-Log([string]$Message) {
    $parent = Split-Path -Parent $LogPath
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    Add-Content -LiteralPath $LogPath -Value ("[" + (Get-Date -Format "s") + "] " + $Message)
}

try {
    Write-Log "Preparing update."
    New-Item -ItemType Directory -Path $payload, $backup -Force | Out-Null
    Expand-Archive -LiteralPath $ZipPath -DestinationPath $payload -Force
    $newExe = Get-ChildItem -LiteralPath $payload -Recurse -File -Filter "dromaius.exe" |
        Select-Object -First 1
    if ($null -eq $newExe) { throw "The update archive does not contain dromaius.exe." }
    $source = $newExe.Directory.FullName

    Wait-Process -Id $ProcessId -ErrorAction SilentlyContinue
    Write-Log "Installing update."
    Get-ChildItem -LiteralPath $AppDirectory -File | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $backup -Force
    }
    Get-ChildItem -LiteralPath $source -File | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $AppDirectory -Force
    }
    $success = $true
    Write-Log "Update installed successfully."
} catch {
    Write-Log ("Update failed: " + $_.Exception.Message)
    if (Test-Path -LiteralPath $backup) {
        Get-ChildItem -LiteralPath $backup -File | ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination $AppDirectory -Force
        }
    }
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
    if ($success) { Remove-Item -LiteralPath $ZipPath -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $ExecutablePath) {
        Start-Process -FilePath $ExecutablePath -WindowStyle Hidden
    }
}
