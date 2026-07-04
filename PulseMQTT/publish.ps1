# PulseMQTT – Single-File Publish
# .\publish.ps1              → Trimmed (~20 MB, empfohlen)
# .\publish.ps1 -Mode NoTrim → ~70 MB, falls Trimming Sensor-Probleme zeigt
# .\publish.ps1 -Mode Fx     → ~8 MB, benötigt .NET 8 Desktop Runtime

param([ValidateSet("Trim","NoTrim","Fx")][string]$Mode = "Trim")

$profile = switch ($Mode) { "Trim" { "SingleFile" } "NoTrim" { "SingleFile_NoTrim" } "Fx" { "FrameworkDependent" } }
dotnet publish PulseMQTT.csproj -p:PublishProfile=$profile

if ($LASTEXITCODE -eq 0) {
    $outDir = switch ($Mode) { "Trim" { "publish" } "NoTrim" { "publish_notrim" } "Fx" { "publish_fxdep" } }
    $size   = [math]::Round((Get-Item "$outDir\PulseMQTT.exe").Length / 1MB, 1)
    Write-Host ""
    Write-Host "✓ .\$outDir\PulseMQTT.exe  ($size MB)" -ForegroundColor Green
} else {
    Write-Host "Build fehlgeschlagen." -ForegroundColor Red
}
