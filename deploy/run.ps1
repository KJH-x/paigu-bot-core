# 长期运行入口（计划任务「登录时」触发）。
# 职责：单实例守卫 -> 切到仓库根（相对路径 config/web/data 依赖 CWD）-> 看护重启 -> 日志 + 14 天保留。
# 注意：绝不向真实群发消息（reply_enabled=false 由 config/app.json 决定）。
$ErrorActionPreference = 'Continue'

$deploy = $PSScriptRoot
$repo = Split-Path -Parent $deploy
Set-Location -LiteralPath $repo

# 可执行文件：优先安装副本，其次 release 构建产物
$exe = Join-Path $deploy 'bin\paigu-bot-core.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    $fallback = Join-Path $repo 'target\release\paigu-bot-core.exe'
    if (Test-Path -LiteralPath $fallback) {
        $exe = $fallback
    } else {
        throw "未找到可执行文件。请先运行 deploy\build.ps1（期望：$exe）"
    }
}

$logDir = Join-Path $deploy 'logs'
New-Item -ItemType Directory -Path $logDir -Force | Out-Null

# 日志保留 14 天
Get-ChildItem -LiteralPath $logDir -Filter 'paigu-*.log' -File -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-14) } |
    Remove-Item -Force -ErrorAction SilentlyContinue

# 单实例守卫：已有实例则退出，不启动第二个
$running = @(Get-Process -Name 'paigu-bot-core' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    $pids = ($running | ForEach-Object { $_.Id }) -join ','
    Write-Host "[run] 已有实例在运行（PID $pids），退出。"
    exit 0
}

if (-not $env:RUST_LOG) { $env:RUST_LOG = 'info' }

$log = Join-Path $logDir ("paigu-{0}.log" -f (Get-Date).ToString('yyyyMMdd'))
$backoff = 5
$stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
Add-Content -LiteralPath $log -Value "[$stamp] [run] supervisor start; exe=$exe cwd=$repo RUST_LOG=$env:RUST_LOG"

while ($true) {
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Add-Content -LiteralPath $log -Value "[$stamp] [run] launching: $exe run"
    # 经 cmd + chcp 65001 追加原始字节，日志保持 UTF-8（PowerShell 5.1 的 *>> 会写成 UTF-16）
    $inner = 'chcp 65001 >nul & "' + $exe + '" run >> "' + $log + '" 2>&1'
    & cmd.exe /d /c $inner
    $code = $LASTEXITCODE
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Add-Content -LiteralPath $log -Value "[$stamp] [run] exited code=$code; restart in ${backoff}s"
    Start-Sleep -Seconds $backoff
    if ($backoff -lt 60) { $backoff = [Math]::Min(60, $backoff * 2) }
}
