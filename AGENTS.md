# AI Agent 开发指南

本文档为 AI 编码助手（如 Claude Code、Cursor、OpenCode）提供项目上下文。

## 项目概述

**LangChainRust Agent** - RAG 对话系统，支持多种压缩策略。

- **主要语言**: Rust
- **Web 框架**: Axum
- **数据库**: SQLite（对话）+ MongoDB（BM25）+ Qdrant（向量）
- **前端**: 单页 HTML + JavaScript

## 关键技术决策

### 为什么用 SQLite？

参考 OpenCode 方案：
- 本地文件，无服务器成本
- Append-only 场景最优
- WAL 模式支持并发读
- 适合对话历史存储

### 为什么用分层压缩？

参考 OpenCode compaction.ts：
- 保护重要设定（用户名字、角色）
- 历史压缩为摘要（节省 token）
- 保留最近对话（连贯性）

## 代码约定

### Rust

```rust
// 使用 thiserror 定义错误
#[derive(Error, Debug)]
pub enum MyError {
    #[error("SQLite 错误: {0}")]
    SqliteError(String),
}

// 异步函数优先
pub async fn do_something(&self) -> Result<(), MyError> {
    // ...
}

// 使用 tracing 日志
tracing::info!("操作完成");
tracing::error!("失败: {:?}", err);
```

### 配置

```toml
# config.toml

[conversation]
max_history_messages = 50    # 最大历史条数
compress_threshold = 15        # 触发压缩阈值
keep_recent_messages = 5       # 保留最近条数
important_keywords = ["我的名字", "我是", "记住", "设定"]
summary_model = "gpt-3.5-turbo"
```

## 核心模块

### conversation_store.rs

**对话存储 + 压缩逻辑**

关键函数：
- `apply_layered_compression()` - 分层压缩
- `apply_summary_compression()` - 摘要压缩
- `generate_summary()` - LLM 摘要生成

### api.rs

**API 服务层**

路由：
- `/api/chat` - 对话
- `/api/search/*` - 搜索
- `/api/documents` - 文档管理

### main.rs

**Axum 服务入口**

## 常见任务

### 添加新压缩模式

1. 编辑 `src/conversation_store.rs`
2. 添加新函数 `apply_xxx_compression()`
3. 在 `apply_compression()` 中添加分支
4. 更新 `get_compress_modes()` 返回列表
5. 前端 `static/index.html` 添加选项

### 修改压缩参数

编辑 `config.toml`：

```toml
[conversation]
max_history_messages = 100
compress_threshold = 30
```

### 添加新 API

1. `src/api.rs` - 添加函数
2. `src/main.rs` - 添加路由
3. 测试：`curl http://localhost:8080/api/new`

## 测试

```bash
cargo test
cargo build --release
```

## 部署

```bash
# 编译
cargo build --release

# 复制到服务器
scp target/release/langchainrust-agent root@server:/opt/

# systemd 服务
sudo systemctl restart langchainrust-agent
```

## 文档位置

技术分析文档：
- `internal/OpenCode对话存储方案.md` - OpenCode 源码分析
- `internal/对话压缩方案研究.md` - 压缩策略
- `internal/开源框架对话存储方案.md` - 框架对比

## 注意事项

### SQLite 并发

- 写入用 `Mutex` 或 `sqlx`（异步）
- WAL 模式：`PRAGMA journal_mode = WAL`

### 压缩触发时机

- 检测 token 数量，超过阈值触发
- 不要每次请求都压缩（浪费 API 调用）

### 配置敏感信息

- `config.toml` 不要提交 API Key
- 使用环境变量：`OPENAI_API_KEY`

## 参考资料

- [OpenCode 源码](internal/OpenCode对话存储方案.md)
- [对话压缩研究](internal/对话压缩方案研究.md)