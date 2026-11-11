# 检查 DEEPSEEK_API_KEY 是否设置，只报长度不报值。
#
# 用法：powershell -NoProfile -File scripts/check-deepseek-key.ps1

$current = $env:DEEPSEEK_API_KEY
if ($current) {
    Write-Output "进程环境: 已设置，长度 $($current.Length)"
} else {
    Write-Output "进程环境: 未设置"
}

$user = [Environment]::GetEnvironmentVariable('DEEPSEEK_API_KEY', 'User')
if ($user) {
    Write-Output "用户变量: 已设置，长度 $($user.Length)"
} else {
    Write-Output "用户变量: 未设置"
}
