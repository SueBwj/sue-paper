$ErrorActionPreference = "Stop"

$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
$cargo = if ($cargoCommand) {
    $cargoCommand.Source
} else {
    Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
}
if (-not (Test-Path -LiteralPath $cargo)) {
    throw "cargo.exe was not found"
}
$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$executable = Join-Path $projectRoot "target\release\sue-paper.exe"
$icon = Join-Path $projectRoot "assets\sue-paper.ico"

& $cargo build --release --bin sue-paper
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& $cargo run --release --bin embed_icon -- $executable $icon
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Built Sue-Paper with embedded icon: $executable"
