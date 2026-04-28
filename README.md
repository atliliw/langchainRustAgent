# LangChainRust Agent

RAG 对话系统，支持多种检索模式和对话压缩策略。

## 功能特性

- **三种检索模式**：向量检索、BM25关键词检索、RRF混合检索
- **对话压缩**：分层压缩、摘要压缩、重要信息保护
- **多格式文档**：支持 TXT、PDF、Markdown、JSON、CSV
- **Web 界面**：单页应用，支持文件上传、搜索、对比测试

## 技术栈

| 组件 | 技术 |
|------|------|
| 后端 | Rust + Axum |
| 向量数据库 | Qdrant |
| BM25 | MongoDB |
| 对话存储 | SQLite |
| Embedding | OpenAI text-embedding-ada-002 |
| LLM | OpenAI GPT-3.5/GPT-4 |

## 项目结构

```
langchainRustAgent/
├── src/                    # 后端源码
│   ├── main.rs             # Axum 入口
│   ├── handlers/           # API handlers
│   ├── stores/             # 存储层
│   ├── services/           # 服务层
│   ├── models/             # 数据模型
│   ├── routes/             # 路由定义
│   └── agents/             # AI agents
├── frontend/               # 前端文件（前后端分离）
│   ├── index.html          # 主页面
│   ├── css/style.css       # 样式
│   └── js/app.js           # JavaScript
└── scripts/                # 部署脚本
```

## 快速开始

```bash
# 1. 配置
cp config.toml.example config.toml
# 编辑 config.toml，填入 OpenAI API Key

# 2. 运行
cargo run --release

# 3. 访问
open http://localhost:8080
```

## 文档

| 文档 | 说明 |
|------|------|
| [部署指南.md](部署指南.md) | 完整部署流程（三种检索模式） |
| [使用指南.md](使用指南.md) | 功能使用说明 |
| [AGENTS.md](AGENTS.md) | AI Agent 开发指南 |

## API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/chat` | POST | 对话（支持压缩策略） |
| `/api/upload` | POST | 上传文档 |
| `/api/search/vector` | POST | 向量检索 |
| `/api/search/bm25` | POST | BM25检索 |
| `/api/search/hybrid` | POST | 混合检索 |
| `/api/search/compare` | POST | 三种检索对比 |
| `/api/stats` | GET | 统计信息 |

## 配置

关键配置项（`config.toml`）：

```toml
[server]
host = "0.0.0.0"
port = 8080

[conversation]
max_history_messages = 50    # 最大历史条数
compress_threshold = 15      # 触发压缩阈值
keep_recent_messages = 5     # 保留最近条数

[openai]
api_key = "your-api-key"
base_url = "https://api.openai.com/v1"
chat_model = "gpt-3.5-turbo"
embedding_model = "text-embedding-ada-002"

[qdrant]
url = "http://localhost:6334"
collection_name = "demo_documents"
```

## License

MIT