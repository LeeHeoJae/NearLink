param([ValidateSet('build', 'test', 'check')][string]$Task = 'build')
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$localCargo = Join-Path $repoRoot '.tools/cargo/bin/cargo.exe'
if (Test-Path $localCargo) {
    $env:CARGO_HOME = Join-Path $repoRoot '.tools/cargo'
    $env:RUSTUP_HOME = Join-Path $repoRoot '.tools/rustup'
    $cargo = $localCargo
} else {
    $cargo = (Get-Command cargo -ErrorAction Stop).Source
}
Push-Location $PSScriptRoot
try {
    switch ($Task) {
        'build' { & $cargo build --release --locked }
        'test' { & $cargo test --locked }
        'check' { & $cargo clippy --all-targets --locked -- -D warnings }
    }
    if ($LASTEXITCODE -ne 0) { throw "Cargo $Task failed ($LASTEXITCODE)" }
} finally { Pop-Location }
