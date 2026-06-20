param(
    [string]$BaseUrl = "http://127.0.0.1:3000",
    [string]$TestAudioDir = "test_audio"
)

$ErrorActionPreference = "Stop"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "音频版权检测系统测试脚本" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

function Test-Health {
    Write-Host "[1/5] 测试健康检查接口..." -ForegroundColor Yellow
    try {
        $response = Invoke-RestMethod -Uri "$BaseUrl/api/health" -Method Get
        Write-Host "  状态: $($response.status)" -ForegroundColor Green
        Write-Host "  版权库大小: $($response.library_size)" -ForegroundColor Green
        Write-Host "  健康检查: 通过" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "  健康检查: 失败 - $_" -ForegroundColor Red
        return $false
    }
}

function Test-ListTracks {
    Write-Host ""
    Write-Host "[2/5] 测试获取版权库列表..." -ForegroundColor Yellow
    try {
        $response = Invoke-RestMethod -Uri "$BaseUrl/api/library" -Method Get
        Write-Host "  总曲目数: $($response.total)" -ForegroundColor Green
        Write-Host "  获取列表: 通过" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "  获取列表: 失败 - $_" -ForegroundColor Red
        return $false
    }
}

function Test-AddTrack {
    param([string]$FilePath, [string]$Title, [string]$Artist)
    
    Write-Host ""
    Write-Host "[3/5] 添加曲目到版权库: $Title" -ForegroundColor Yellow
    
    if (-not (Test-Path $FilePath)) {
        Write-Host "  文件不存在: $FilePath" -ForegroundColor Red
        return $false
    }
    
    try {
        $fileBytes = [System.IO.File]::ReadAllBytes($FilePath)
        $fileEnc = [System.Text.Encoding]::GetEncoding('ISO-8859-1').GetString($fileBytes)
        $boundary = [System.Guid]::NewGuid().ToString()
        $LF = "`r`n"

        $bodyLines = @(
            "--$boundary",
            "Content-Disposition: form-data; name=`"audio`"; filename=`"$([System.IO.Path]::GetFileName($FilePath))`"",
            "Content-Type: audio/mpeg",
            "",
            $fileEnc,
            "--$boundary--",
            ""
        ) -join $LF

        $url = "$BaseUrl/api/library?title=$([System.Uri]::EscapeDataString($Title))&artist=$([System.Uri]::EscapeDataString($Artist))"
        
        $response = Invoke-RestMethod -Uri $url -Method Post `
            -ContentType "multipart/form-data; boundary=`"$boundary`"" `
            -Body $bodyLines
        
        Write-Host "  曲目ID: $($response.track_id)" -ForegroundColor Green
        Write-Host "  标题: $($response.title)" -ForegroundColor Green
        Write-Host "  艺术家: $($response.artist)" -ForegroundColor Green
        Write-Host "  时长: $($response.duration)秒" -ForegroundColor Green
        Write-Host "  指纹数: $($response.fingerprint_count)" -ForegroundColor Green
        Write-Host "  添加曲目: 通过" -ForegroundColor Green
        
        return $response.track_id
    } catch {
        Write-Host "  添加曲目: 失败 - $_" -ForegroundColor Red
        return $false
    }
}

function Test-Detect {
    param([string]$FilePath, [string]$Description)
    
    Write-Host ""
    Write-Host "[4/5] 侵权检测: $Description" -ForegroundColor Yellow
    
    if (-not (Test-Path $FilePath)) {
        Write-Host "  文件不存在: $FilePath" -ForegroundColor Red
        return $false
    }
    
    try {
        $fileBytes = [System.IO.File]::ReadAllBytes($FilePath)
        $fileEnc = [System.Text.Encoding]::GetEncoding('ISO-8859-1').GetString($fileBytes)
        $boundary = [System.Guid]::NewGuid().ToString()
        $LF = "`r`n"

        $bodyLines = @(
            "--$boundary",
            "Content-Disposition: form-data; name=`"audio`"; filename=`"$([System.IO.Path]::GetFileName($FilePath))`"",
            "Content-Type: audio/mpeg",
            "",
            $fileEnc,
            "--$boundary--",
            ""
        ) -join $LF

        $url = "$BaseUrl/api/detect"
        
        $response = Invoke-RestMethod -Uri $url -Method Post `
            -ContentType "multipart/form-data; boundary=`"$boundary`"" `
            -Body $bodyLines
        
        Write-Host "  是否侵权: $($response.is_infringing)" -ForegroundColor $(if ($response.is_infringing) { "Red" } else { "Green" })
        Write-Host "  置信度: $([math]::Round($response.confidence * 100, 2))%" -ForegroundColor Cyan
        Write-Host "  处理时间: $($response.processing_time_ms)ms" -ForegroundColor Cyan
        
        if ($response.matched_track) {
            Write-Host "  匹配曲目: $($response.matched_track.title) - $($response.matched_track.artist)" -ForegroundColor Yellow
        }
        
        if ($response.match_segments.Count -gt 0) {
            Write-Host "  匹配片段数: $($response.match_segments.Count)" -ForegroundColor Cyan
            foreach ($seg in $response.match_segments) {
                Write-Host "    - 查询: $($seg.query_start)s - $($seg.query_end)s, 曲目: $($seg.track_start)s - $($seg.track_end)s, 置信度: $([math]::Round($seg.confidence * 100, 2))%" -ForegroundColor Gray
            }
        }
        
        Write-Host "  侵权检测: 通过" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "  侵权检测: 失败 - $_" -ForegroundColor Red
        return $false
    }
}

function Test-DeleteTrack {
    param([string]$TrackId)
    
    Write-Host ""
    Write-Host "[5/5] 删除测试曲目..." -ForegroundColor Yellow
    
    if (-not $TrackId) {
        Write-Host "  无曲目ID，跳过删除" -ForegroundColor Yellow
        return $true
    }
    
    try {
        $response = Invoke-WebRequest -Uri "$BaseUrl/api/library/$TrackId" -Method Delete
        if ($response.StatusCode -eq 204) {
            Write-Host "  删除曲目: 通过" -ForegroundColor Green
            return $true
        } else {
            Write-Host "  删除曲目: 失败 - 状态码 $($response.StatusCode)" -ForegroundColor Red
            return $false
        }
    } catch {
        Write-Host "  删除曲目: 失败 - $_" -ForegroundColor Red
        return $false
    }
}

$success = 0
$total = 5

if (Test-Health) { $success++ }
if (Test-ListTracks) { $success++ }

$testAudioPath = Join-Path $TestAudioDir "test_track_1.wav"
$trackId = Test-AddTrack -FilePath $testAudioPath -Title "测试曲目1" -Artist "测试艺术家"
if ($trackId) { $success++ }

$detectPath = Join-Path $TestAudioDir "test_track_1.wav"
if (Test-Detect -FilePath $detectPath -Description "版权库内曲目检测") { $success++ }

if (Test-DeleteTrack -TrackId $trackId) { $success++ }

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "测试完成: $success / $total 通过" -ForegroundColor $(if ($success -eq $total) { "Green" } else { "Yellow" })
Write-Host "========================================" -ForegroundColor Cyan
