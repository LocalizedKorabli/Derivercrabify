param(
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$root = (Get-Location).Path

# 从 Cargo.toml 读取版本号
$cargo = [System.IO.File]::ReadAllText("$root\src-tauri\Cargo.toml")
$version = if ($cargo -match 'version\s*=\s*"([\d.]+)"') { $Matches[1] } else { throw "Version not found in Cargo.toml" }
$tag = "v$version"

Write-Host "=== Syncing version to package.json ===" -ForegroundColor Cyan
node "$root\scripts\sync-version.js"
if ($LASTEXITCODE -ne 0) { throw "Sync version failed" }

Write-Host "=== Building v$version ===" -ForegroundColor Cyan
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

# 定位最新的安装包
$exe = Get-ChildItem -Path "$root\target\release\bundle\nsis\*_x64-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $exe) { throw "Installer not found" }
Write-Host "Installer: $($exe.Name)" -ForegroundColor Cyan

# 生成 metadata.json
$meta = @{ version = $version; path = "https://dl.localizedkorabli.org/derivercrabify/app/Derivercrabify_${version}_x64-setup.exe" }
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText("$root\metadata.json", ($meta | ConvertTo-Json), $utf8NoBom)
Write-Host "Metadata: $root\metadata.json" -ForegroundColor Cyan

if ($DryRun) {
    Write-Host "Dry-run: would create release $tag" -ForegroundColor Yellow
    exit 0
}

# 提交并推送版本号变更
git add src-tauri\Cargo.toml package.json metadata.json
git diff --cached --quiet
if ($LASTEXITCODE -eq 1) {
    git commit -m "chore: bump version to $version"
    git pull --rebase origin main
    git push origin main
    if ($LASTEXITCODE -ne 0) { throw "Push failed" }
} else {
    Write-Host "No version changes to commit" -ForegroundColor Yellow
}

# 创建 GitHub Release
gh release create $tag "$($exe.FullName)" `
    --title "$tag" `
    --notes "See CHANGELOG or commit history for details." `
    --target main

if ($LASTEXITCODE -eq 0) {
    Write-Host "Release $tag created successfully!" -ForegroundColor Green
    Write-Host "GitHub Actions will now publish to R2 and update metadata." -ForegroundColor Cyan
}
