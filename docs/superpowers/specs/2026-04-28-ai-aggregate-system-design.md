# AI Agent 信息聚合系统设计文档

## 一、项目概述

**目标**：构建一个AI Agent技术信息聚合系统，用于个人学习研究，跟踪AI Agent领域前沿动态。

**核心功能**：
- 多数据源采集（GitHub、Hacker News、RSS博客、ArXiv、Twitter）
- LLM智能摘要
- 向量智能检索
- 多维度展示（时间流、分类卡片、关键词标签）

**架构**：前后端分离，基于现有Rust项目扩展，使用langchainrust框架构建Agent工具。

---

## 二、系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                      前端展示层                               │
│  http://192.168.10.100:8080                                 │
│                                                             │
│  时间流列表  │  分类卡片  │  AI摘要  │  关键词标签            │
└─────────────────────────┬───────────────────────────────────┘
                          │ API
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                      Rust后端 (8090)                         │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │ 采集调度器   │  │ 内容处理器   │  │ 存储服务    │          │
│  │ (Agent)     │  │ (LLM)       │  │             │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│                                                             │
│  Agent Tools:                                               │
│  ├── GitHub Tool      → 获取 trending repo                  │
│  ├── HackerNews Tool  → 获取 top stories                    │
│  ├── RSS Tool         → 订阅博客 RSS                         │
│  ├── ArXiv Tool       → 获取最新论文                         │
│  └── Twitter Tool     → 获取用户推文                         │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                      数据存储层                              │
│                                                             │
│  Qdrant      → 内容向量存储（智能检索）                       │
│  MongoDB     → 元数据存储                                   │
│  SQLite      → 采集记录                                     │
└─────────────────────────────────────────────────────────────┘
```

---

## 三、数据采集Agent设计

### 3.1 GitHub Agent

**数据源**：GitHub Trending API（无需认证）

**采集内容**：
- trending repositories（按语言筛选）
- 项目名称、描述、URL、star数、语言
- 主要贡献者

**实现**：
```rust
pub struct GitHubTool {
    api_base: String,  // https://api.github.com
}

impl GitHubTool {
    pub async fn fetch_trending(&self, language: &str) -> Result<Vec<RepoInfo>>;
    pub async fn fetch_repo_details(&self, repo_url: &str) -> Result<RepoDetails>;
}
```

### 3.2 Hacker News Agent

**数据源**：Hacker News Algolia API（免费）

**采集内容**：
- top stories（按时间范围）
- AI相关话题（关键词过滤：AI、agent、LLM、GPT）
- 评论数、点赞数

**实现**：
```rust
pub struct HackerNewsTool {
    api_base: String,  // https://hn.algolia.com/api/v1
}

impl HackerNewsTool {
    pub async fn fetch_top_stories(&self, tags: &[&str]) -> Result<Vec<HNStory>>;
}
```

### 3.3 RSS Agent

**数据源**：知名博客RSS订阅

**订阅列表**：
- OpenAI Blog: https://openai.com/blog/rss.xml
- Anthropic Blog: https://www.anthropic.com/index/rss.xml
- Andrej Karpathy Blog
- Google AI Blog
- DeepMind Blog
- Microsoft AI Blog

**实现**：
```rust
pub struct RSSTool {
    feeds: Vec<String>,
}

impl RSSTool {
    pub async fn fetch_feed(&self, feed_url: &str) -> Result<Vec<RSSItem>>;
    pub async fn fetch_all_feeds(&self) -> Result<Vec<RSSItem>>;
}
```

### 3.4 ArXiv Agent

**数据源**：ArXiv API（免费）

**采集内容**：
- cs.AI（人工智能）
- cs.CL（计算与语言）
- cs.LG（机器学习）
- 论文标题、摘要、作者、PDF链接

**实现**：
```rust
pub struct ArXivTool {
    api_base: String,  // http://export.arxiv.org/api/query
}

impl ArXivTool {
    pub async fn fetch_papers(&self, category: &str, max_results: usize) -> Result<Vec<PaperInfo>>;
}
```

### 3.5 Twitter Agent（需付费API）

**数据源**：Twitter API v2（需要Bearer Token）

**采集用户**：
- @karpathy (Andrej Karpathy)
- @sama (Sam Altman)
- @ylecun (Yann LeCun)
- OpenAI (@OpenAI)
- Anthropic (@AnthropicAI)

**实现**：
```rust
pub struct TwitterTool {
    bearer_token: String,
}

impl TwitterTool {
    pub async fn fetch_user_tweets(&self, username: &str, count: usize) -> Result<Vec<TweetInfo>>;
}
```

---

## 四、数据存储模型

### 4.1 内容存储结构

```rust
pub struct AggregatedContent {
    pub id: String,               // UUID
    pub source: ContentSource,    // 数据来源枚举
    pub title: String,            // 标题
    pub content: String,          // 正文内容
    pub url: String,              // 原始链接
    pub author: Option<String>,   // 作者
    pub published_at: i64,        // 发布时间
    pub collected_at: i64,        // 采集时间
    pub summary: Option<String>,  // LLM摘要
    pub keywords: Vec<String>,    // 关键词标签
    pub metadata: HashMap<String, String>,  // 扩展元数据
}

pub enum ContentSource {
    GitHub,
    HackerNews,
    RSS,
    ArXiv,
    Twitter,
}
```

### 4.2 向量存储

使用Qdrant存储内容向量，支持智能检索：
- 向量维度：1536（text-embedding-ada-002）
- 集合名称：`ai_content`
- 元数据：source、title、published_at

### 4.3 SQLite采集记录

```sql
CREATE TABLE collection_record (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    collected_at INTEGER NOT NULL,
    items_count INTEGER,
    status TEXT,  -- success/failed
    error_message TEXT
);
```

---

## 五、API设计

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/aggregate/collect` | POST | 触发采集 |
| `/api/aggregate/list` | GET | 获取内容列表 |
| `/api/aggregate/search` | POST | 智能搜索 |
| `/api/aggregate/summary/:id` | GET | 获取AI摘要 |
| `/api/aggregate/keywords` | GET | 获取热门关键词 |
| `/api/aggregate/stats` | GET | 采集统计 |

### 5.1 触发采集

```json
POST /api/aggregate/collect
{
    "sources": ["github", "hackernews", "rss"],  // 可选，默认全部
    "force": false  // 是否强制采集（忽略缓存）
}

Response:
{
    "success": true,
    "collected_count": 50,
    "records": [
        {"source": "github", "count": 10, "status": "success"},
        {"source": "hackernews", "count": 20, "status": "success"},
        {"source": "rss", "count": 20, "status": "success"}
    ]
}
```

### 5.2 内容列表

```json
GET /api/aggregate/list?source=github&limit=20&offset=0

Response:
{
    "total": 100,
    "items": [
        {
            "id": "xxx",
            "source": "github",
            "title": "langchain-rust",
            "content": "...",
            "url": "https://github.com/...",
            "author": "bob",
            "published_at": 1234567890,
            "summary": "LLM应用开发框架...",
            "keywords": ["LLM", "agent", "rust"]
        }
    ]
}
```

### 5.3 智能搜索

```json
POST /api/aggregate/search
{
    "query": "AI Agent 编程框架",
    "top_k": 10
}

Response:
{
    "results": [
        {
            "id": "xxx",
            "title": "...",
            "content": "...",
            "score": 0.85,
            "summary": "..."
        }
    ]
}
```

---

## 六、LLM内容处理

### 6.1 自动摘要

使用GPT-3.5-turbo生成内容摘要（200字以内）：

```rust
pub async fn generate_summary(&self, content: &str) -> Result<String> {
    let prompt = format!(
        "请用中文简要总结以下AI/Agent相关内容（不超过200字）：\n\n{}", 
        content
    );
    // 调用LLM
}
```

### 6.2 关键词提取

提取3-5个核心关键词：

```rust
pub async fn extract_keywords(&self, content: &str) -> Result<Vec<String>> {
    let prompt = format!(
        "请从以下内容中提取3-5个AI/Agent相关的核心关键词：\n\n{}",
        content
    );
    // 调用LLM
}
```

---

## 七、前端展示设计

### 7.1 时间流列表

- 按采集时间倒序排列
- 显示来源图标、标题、摘要、关键词
- 点击展开完整内容

### 7.2 分类卡片

- 按数据源分类（GitHub卡片、HN卡片、RSS卡片...）
- 每个卡片显示最新内容概览

### 7.3 AI摘要面板

- 点击内容显示AI生成的摘要
- 支持重新生成摘要

### 7.4 关键词标签导航

- 显示热门关键词（如：LLM、Agent、LangChain、Rust）
- 点击关键词过滤相关内容

---

## 八、实现优先级

### 第一版（最小可用）

- GitHub Agent（trending repos）
- Hacker News Agent（top stories）
- RSS Agent（3-5个博客订阅）
- 基础存储（SQLite + Qdrant）
- 简单时间流列表展示
- 手动触发采集按钮

### 第二版（智能增强）

- ArXiv Agent
- LLM自动摘要
- LLM关键词提取
- 向量智能检索
- 分类卡片视图

### 第三版（完整体验）

- Twitter Agent（需付费API）
- 关键词标签导航
- 采集历史统计
- 内容去重优化

---

## 九、代码结构扩展

```
src/
├── agents/               # 新增：Agent采集模块
│   ├── mod.rs
│   ├── github.rs         # GitHub Agent
│   ├── hackernews.rs     # Hacker News Agent
│   ├── rss.rs            # RSS Agent
│   ├── arxiv.rs          # ArXiv Agent
│   └── twitter.rs        # Twitter Agent
│
├── models/
│   ├── aggregate.rs      # 新增：聚合内容模型
│   └── ...
│
├── handlers/
│   ├── aggregate.rs      # 新增：聚合API处理
│   └── ...
│
├── stores/
│   ├── content_store.rs  # 新增：内容存储
│   └── ...
│
└── services/
    ├── aggregate_service.rs  # 新增：聚合服务
    └── ...
```

---

## 十、配置扩展

```toml
# config.toml 新增配置

[aggregate]
# 采集配置
github_language = "rust,python"   # GitHub trending语言过滤
hn_keywords = ["ai", "agent", "llm", "gpt"]  # HN关键词过滤
rss_feeds = [
    "https://openai.com/blog/rss.xml",
    "https://www.anthropic.com/index/rss.xml",
]
arxiv_categories = ["cs.AI", "cs.CL", "cs.LG"]
arxiv_max_results = 20

# Twitter配置（可选）
twitter_bearer_token = ""
twitter_users = ["karpathy", "sama", "ylecun"]

# LLM处理配置
summary_model = "gpt-3.5-turbo"
summary_max_tokens = 200
keyword_count = 5
```

---

## 十一、风险与限制

1. **Twitter API需付费**：基础版$100/月，可能跳过Twitter源
2. **GitHub API限流**：未认证每小时60次，需控制采集频率
3. **RSS稳定性**：部分博客RSS可能不可用，需容错处理
4. **LLM成本**：大量摘要生成会增加API调用成本

---

## 十二、总结

本项目将扩展现有Rust后端，使用langchainrust框架构建多数据源采集Agent系统，实现AI Agent技术信息的聚合、智能处理和多维度展示。

**下一步**：实现第一版（GitHub + Hacker News + RSS + 基础展示）