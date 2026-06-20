# 短视频音频版权检测系统

基于 Rust + Axum 实现的短视频音频版权检测系统，使用音频指纹算法（类似 Shazam）实现快速、准确的版权识别。

## 功能特性

- 🔊 **多格式支持**: 支持 MP3、WAV、AAC、MP4 等常见音频格式
- 🎵 **音频指纹**: 基于 FFT 频谱分析和峰值匹配的指纹算法
- ⚡ **快速检测**: 哈希索引 + 时间差直方图匹配，检测速度快
- 📚 **版权库管理**: RESTful API 管理版权音频库
- 🔍 **片段检测**: 支持检测音频片段是否侵权
- 📊 **详细结果**: 返回匹配片段、置信度等详细信息

## 技术架构

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Axum Web API   │────▶│  音频解码模块   │────▶│  指纹生成模块   │
└─────────────────┘     └─────────────────┘     └─────────────────┘
         │                                            │
         │                                            ▼
         │                                     ┌─────────────────┐
         └────────────────────────────────────▶│  版权库管理     │
                                               │  (哈希索引)     │
                                               └─────────────────┘
```

## 项目结构

```
src/
├── main.rs              # 程序入口
├── lib.rs               # 库文件，定义 AppState
├── errors.rs            # 错误类型定义
├── models.rs            # 数据模型
├── library.rs           # 版权音频库管理
├── audio/
│   ├── mod.rs           # 音频模块导出
│   ├── decoder.rs       # 音频解码器（基于 Symphonia）
│   └── fingerprint.rs   # 音频指纹生成与匹配算法
├── routes/
│   ├── mod.rs           # 路由模块
│   ├── health.rs        # 健康检查接口
│   ├── library.rs       # 版权库管理接口
│   └── detect.rs        # 侵权检测接口
└── bin/
    └── generate_test_audio.rs  # 测试音频生成工具
```

## 核心算法

### 音频指纹生成

1. **重采样**: 将音频统一重采样到 22050Hz 单声道
2. **分帧**: 使用 2048 点帧长，512 点帧移
3. **窗函数**: 汉宁窗减少频谱泄漏
4. **FFT**: 快速傅里叶变换得到频谱
5. **峰值提取**: 在频域中提取局部峰值点
6. **哈希生成**: 对峰值对（f1, f2, Δt）生成哈希指纹

### 版权匹配

1. **哈希索引**: 使用倒排索引快速查找候选曲目
2. **时间差直方图**: 统计匹配对的时间差，找到最佳对齐
3. **置信度计算**: 基于匹配数量和质量计算置信度
4. **片段提取**: 提取连续匹配的时间片段

## API 接口

### 健康检查

```
GET /api/health
```

响应:
```json
{
  "status": "ok",
  "library_size": 10
}
```

### 版权库管理

**获取曲目列表**
```
GET /api/library
```

**获取单曲信息**
```
GET /api/library/:id
```

**添加版权曲目**
```
POST /api/library?title=歌曲名&artist=艺术家
Content-Type: multipart/form-data

audio: <音频文件>
```

响应:
```json
{
  "track_id": "uuid",
  "title": "歌曲名",
  "artist": "艺术家",
  "duration": 180.5,
  "fingerprint_count": 15000
}
```

**删除曲目**
```
DELETE /api/library/:id
```

### 侵权检测

**检测音频是否侵权**
```
POST /api/detect
Content-Type: multipart/form-data

audio: <音频文件>
```

响应:
```json
{
  "is_infringing": true,
  "confidence": 0.85,
  "matched_track": {
    "id": "uuid",
    "title": "版权歌曲",
    "artist": "艺术家",
    "duration": 200.0,
    "fingerprint_count": 20000,
    "created_at": 1234567890
  },
  "match_segments": [
    {
      "query_start": 5.0,
      "query_end": 25.0,
      "track_start": 10.0,
      "track_end": 30.0,
      "confidence": 0.9
    }
  ],
  "processing_time_ms": 150
}
```

## 使用说明

### 编译运行

```bash
# 开发模式运行
cargo run

# 发布版本编译
cargo build --release

# 运行发布版本
./target/release/audio-copyright-detector
```

服务默认监听 `127.0.0.1:3000`。

### 生成测试音频

```bash
cargo run --bin generate_test_audio
```

这会在 `test_audio/` 目录下生成几个测试用的 WAV 文件。

### 运行测试

```powershell
# PowerShell 测试脚本
.\test.ps1
```

## 配置说明

可通过环境变量配置:

- `RUST_LOG`: 日志级别（默认: `info`）

## 依赖

- `axum`: Web 框架
- `tokio`: 异步运行时
- `symphonia`: 音频解码
- `rustfft`: 快速傅里叶变换
- `hound`: WAV 文件读写
- `uuid`: 唯一标识符
- `serde/serde_json`: 序列化
- `parking_lot`: 高性能锁

## 性能特点

- 使用倒排索引实现 O(1) 哈希查找
- 时间差直方图算法抗噪性强
- 支持部分片段匹配
- 内存中的版权库，检测速度快

## 注意事项

1. 目前版权库存储在内存中，服务重启后数据会丢失
2. 建议使用时将版权库持久化到数据库（如 SQLite、PostgreSQL）
3. 音频指纹算法参数可根据实际场景调整
4. 置信度阈值可在 `CopyrightLibrary::with_threshold()` 中设置
