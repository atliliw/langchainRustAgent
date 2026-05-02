//! 存储层
//!
//! qdrant        向量存储（Qdrant 数据库）：语义检索用
//! bm25          BM25 关键词检索（MongoDB 持久化）：精准匹配用
//! hybrid        RRF 混合检索（BM25 + 向量融合）
//! conversation  对话历史（SQLite）+ 4种压缩策略
//! content_store Agent 采集数据存储（SQLite）

pub mod qdrant;           // 向量存储
pub mod bm25;             // BM25 检索
pub mod hybrid;           // 混合检索
pub mod conversation;     // 对话历史
pub mod content_store;    // Agent 采集数据

pub use qdrant::QdrantStore;
pub use bm25::BM25Store;
pub use bm25::DocumentFileInfo;
pub use hybrid::HybridStore;
pub use hybrid::HybridSearchResult;
pub use conversation::ConversationStore;
pub use conversation::ApiStatsSummary;
pub use conversation::ApiTypeStats;
pub use conversation::RecentCall;
pub use conversation::estimate_tokens;
pub use content_store::ContentStore;
