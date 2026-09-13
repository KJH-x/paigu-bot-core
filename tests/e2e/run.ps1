$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Resolve-Path (Join-Path $PSScriptRoot '..\..'))
node tests/e2e/sim.mjs
exit $LASTEXITCODE
