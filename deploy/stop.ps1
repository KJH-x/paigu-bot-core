# 停止长期运行实例：先停计划任务（结束看护进程，避免其重启），再清理残留 exe 进程。
$ErrorActionPreference = 'Continue'

$taskName = 'paigu-bot-core-run'
$task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($task) {
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Write-Host "[stop] 已请求停止计划任务 $taskName"
} else {
    Write-Host "[stop] 未找到计划任务 $taskName（可能仅手工启动）"
}

Get-Process -Name 'paigu-bot-core' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800

$left = @(Get-Process -Name 'paigu-bot-core' -ErrorAction SilentlyContinue)
if ($left.Count -gt 0) {
    Write-Host "[stop] 残留进程：$((($left | ForEach-Object { $_.Id }) -join ','))"
} else {
    Write-Host "[stop] 已停止"
}
