//! PageIndex SQLite 存储
//!
//! 按标题层级构建文档树，每个节点存一行。

use crate::config::Config;
use crate::errors::StoreError;
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PNode {
    pub node_id: String,
    pub title: String,
    pub level: usize,
    pub content: String,
    pub children: Vec<PNode>,
}

/// 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageIndexSearchResult {
    pub doc_id: String,
    pub doc_title: String,
    pub node_id: String,
    pub title: String,
    pub content_preview: String,
    pub path: String,
    pub level: usize,
    pub summary: String,
}

pub struct PageIndexStore {
    pub pool: SqlitePool,
}

impl PageIndexStore {
    pub async fn new(_config: &Config) -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect("sqlite:pageindex.db?mode=rwc")
            .await
            .map_err(|e| StoreError::ConnectionError(e.to_string()))?;
        let store = Self { pool };
        store.create_tables().await?;
        Ok(store)
    }

    async fn create_tables(&self) -> Result<(), StoreError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pageindex_docs (
                doc_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"
        ).execute(&self.pool).await.map_err(|e| StoreError::CreateTableError(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pageindex_nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                parent_node_id TEXT,
                title TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                level INTEGER NOT NULL DEFAULT 0,
                path TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                FOREIGN KEY (doc_id) REFERENCES pageindex_docs(doc_id)
            )"
        ).execute(&self.pool).await.map_err(|e| StoreError::CreateTableError(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_nodes_doc_id ON pageindex_nodes(doc_id)"
        ).execute(&self.pool).await.ok();

        Ok(())
    }

    /// 构建并存储文档树
    pub async fn build_tree(&self, doc_id: &str, title: &str, text: &str) -> Result<(), StoreError> {
        // 先清除旧数据
        self.delete_doc(doc_id).await.ok();

        // 存 doc
        sqlx::query("INSERT OR REPLACE INTO pageindex_docs (doc_id, title) VALUES (?, ?)")
            .bind(doc_id).bind(title)
            .execute(&self.pool).await.ok();

        // 用栈遍历，避免递归的 lifetime 问题
        struct NodeItem {
            node_id: String, parent_node_id: Option<String>,
            title: String, content: String, level: i32, path: String,
        }

        let root = parse_tree(doc_id, title, text);
        let mut stack = vec![(root, String::new(), None)]; // (node, parent_path, parent_node_id)

        while let Some((node, parent_path, parent_id)) = stack.pop() {
            let path = if parent_path.is_empty() {
                format!("/{}", node.title)
            } else {
                format!("{}/{}", parent_path, node.title)
            };

            sqlx::query(
                "INSERT INTO pageindex_nodes (doc_id, node_id, parent_node_id, title, content, level, path, summary) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(doc_id).bind(&node.node_id).bind(&parent_id)
            .bind(&node.title).bind(&node.content).bind(node.level as i32).bind(&path).bind("")
            .execute(&self.pool).await.ok();

            // 子节点入栈（逆序以保持顺序）
            for child in node.children.into_iter().rev() {
                stack.push((child, path.clone(), Some(node.node_id.clone())));
            }
        }

        Ok(())
    }

    /// 搜索 PageIndex 文档（跨文档全文检索）
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<PageIndexSearchResult>, StoreError> {
        let like = format!("%{}%", query);
        let results = sqlx::query_as::<_, (String, String, String, String, String, String, i32, String)>(
            "SELECT n.doc_id, d.title, n.title, n.content, n.path, n.node_id, n.level, n.summary
             FROM pageindex_nodes n
             JOIN pageindex_docs d ON d.doc_id = n.doc_id
             WHERE n.content LIKE ? OR n.title LIKE ?
             ORDER BY n.level ASC
             LIMIT ?"
        )
        .bind(&like).bind(&like).bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::SearchError(e.to_string()))?;

        Ok(results.into_iter().map(|r| PageIndexSearchResult {
            doc_id: r.0, doc_title: r.1, title: r.2,
            content_preview: r.3.chars().take(200).collect(),
            path: r.4, node_id: r.5, level: r.6 as usize,
            summary: r.7,
        }).collect())
    }

    /// 列出所有 PageIndex 文档
    pub async fn list_docs(&self) -> Result<Vec<(String, String)>, StoreError> {
        let rows = sqlx::query("SELECT doc_id, title FROM pageindex_docs ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::SearchError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// 删除文档
    pub async fn delete_doc(&self, doc_id: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM pageindex_nodes WHERE doc_id = ?").bind(doc_id)
            .execute(&self.pool).await.ok();
        sqlx::query("DELETE FROM pageindex_docs WHERE doc_id = ?").bind(doc_id)
            .execute(&self.pool).await.ok();
        Ok(())
    }
}

/// 按标题层级解析文档树
pub fn parse_tree(doc_id: &str, title: &str, text: &str) -> PNode {
    let mut root = PNode {
        node_id: format!("{}_root", doc_id),
        title: title.to_string(),
        level: 0,
        content: String::new(),
        children: vec![],
    };
    let mut stack: Vec<(String, usize)> = vec![]; // (node_id, level)
    let mut ct = String::new();
    let mut cs = String::new();
    let mut cl = 0;

    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            if !ct.is_empty() {
                let node = PNode {
                    node_id: format!("{}_{}", doc_id, ct.replace(&[' ', '#'][..], "_")),
                    title: ct.clone(), level: cl, content: cs.clone(), children: vec![],
                };
                insert_child(&mut root, &mut stack, node, cl);
            }
            cl = t.chars().take_while(|c| *c == '#').count();
            ct = t.trim_start_matches('#').trim().to_string();
            cs.clear();
        } else if !t.is_empty() {
            if !cs.is_empty() { cs.push('\n'); }
            cs.push_str(t);
        }
    }
    if !ct.is_empty() {
        let node = PNode {
            node_id: format!("{}_{}", doc_id, ct.replace(&[' ', '#'][..], "_")),
            title: ct, level: cl, content: cs, children: vec![],
        };
        insert_child(&mut root, &mut stack, node, cl);
    }
    root
}

fn insert_child(root: &mut PNode, stack: &mut Vec<(String, usize)>, node: PNode, level: usize) {
    while let Some(&(ref _id, l)) = stack.last() {
        if l < level { break; }
        stack.pop();
    }
    if let Some(&(ref parent_id, _)) = stack.last() {
        if let Some(parent) = find_node_mut(root, parent_id) {
            parent.children.push(node);
            let idx = parent.children.len() - 1;
            stack.push((parent.children[idx].node_id.clone(), level));
        }
    } else {
        root.children.push(node);
        let idx = root.children.len() - 1;
        stack.push((root.children[idx].node_id.clone(), level));
    }
}

fn find_node_mut<'a>(node: &'a mut PNode, node_id: &str) -> Option<&'a mut PNode> {
    if node.node_id == node_id { return Some(node); }
    for child in &mut node.children {
        if let Some(found) = find_node_mut(child, node_id) { return Some(found); }
    }
    None
}
