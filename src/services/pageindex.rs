use crate::config::Config;
use crate::stores::pageindex_store::{PageIndexStore, parse_tree};
use serde::{Deserialize, Serialize};
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};

#[derive(Deserialize)]
pub struct BuildRequest {
    pub doc_id: String,
    pub title: String,
    pub text: String,
}

#[derive(Deserialize)]
pub struct SearchRequest {
    pub doc_id: String,
    pub query: String,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub result: String,
    pub path: Vec<String>,
}

#[derive(Serialize)]
pub struct PageIndexInfo {
    pub doc_id: String,
    pub title: String,
    pub node_count: usize,
}

pub struct PageIndex;

impl PageIndex {
    /// 构建树并存入 SQLite，如果有 LLM 则生成节点摘要
    pub async fn build_from_text(store: &PageIndexStore, doc_id: &str, title: &str, text: &str, llm: Option<&OpenAIChat>) -> Result<PageIndexInfo, String> {
        store.build_tree(doc_id, title, text).await.map_err(|e| e.to_string())?;

        if let Some(llm) = llm {
            if let Ok(tree) = load_tree(store, doc_id).await {
                generate_summaries(store, doc_id, &tree, llm).await;
            }
        }

        let root = parse_tree(doc_id, title, text);
        let node_count = count_nodes(&root);
        Ok(PageIndexInfo { doc_id: doc_id.to_string(), title: title.to_string(), node_count })
    }

    /// LLM 导航搜索（从 SQLite 读取）
    pub async fn search(config: &Config, store: &PageIndexStore, req: &SearchRequest) -> Result<SearchResponse, String> {
        // 从 SQLite 加载树
        let doc = load_tree(store, &req.doc_id).await?;

        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(512));
        let mut path = vec!["根".to_string()];
        let mut current_nodes = doc.children.clone();
        let mut result = String::new();

        for _ in 0..3 {
            if current_nodes.is_empty() { break; }
            let list: String = current_nodes.iter().enumerate()
                .map(|(i,n)| format!("{}. {}: {}", i+1, n.title, n.summary))
                .collect::<Vec<_>>().join("\n");
            let p = format!("根据查询选择最相关的（只返回序号）：\n\n查询：{}\n\n{}", req.query, list);
            let resp = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| e.to_string())?;
            let idx = resp.content.trim().parse::<usize>().unwrap_or(1).saturating_sub(1).min(current_nodes.len()-1);
            path.push(current_nodes[idx].title.clone());
            if !current_nodes[idx].content.is_empty() { result = current_nodes[idx].content.clone(); }
            current_nodes = current_nodes[idx].children.clone();
        }
        if result.is_empty() {
            result = doc.children.iter().map(|n| n.content.clone()).collect::<Vec<_>>().join("\n\n");
        }
        Ok(SearchResponse { query: req.query.clone(), result: result.chars().take(500).collect(), path })
    }
}

/// 从 SQLite 重建树
async fn load_tree(store: &PageIndexStore, doc_id: &str) -> Result<RawNode, String> {
    use sqlx::Row;
    // 直接用 pool 查询
    let pool = &store.pool;
    let rows = sqlx::query(
        "SELECT node_id, parent_node_id, title, content, level, summary FROM pageindex_nodes WHERE doc_id = ? ORDER BY id ASC"
    )
    .bind(doc_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("加载失败: {}", e))?;

    let mut nodes: Vec<RawNode> = rows.into_iter().map(|r| RawNode {
        node_id: r.get(0), parent_node_id: r.get(1), title: r.get(2),
        content: r.get(3), level: r.get::<i32,_>(4) as usize, children: vec![],
        summary: r.get(5),
    }).collect();

    // 构建树
    let mut root = RawNode {
        node_id: format!("{}_root", doc_id), parent_node_id: None,
        title: String::new(), content: String::new(), level: 0, children: vec![], summary: String::new(),
    };

    let ids: Vec<String> = nodes.iter().map(|n| n.node_id.clone()).collect();
    for i in (0..nodes.len()).rev() {
        let node = nodes.remove(i);
        if let Some(pid) = &node.parent_node_id {
            if let Some(parent) = nodes.iter_mut().chain(std::iter::once(&mut root))
                .find(|n| n.node_id == *pid) {
                parent.children.push(node);
            } else {
                root.children.push(node);
            }
        } else {
            root.children.push(node);
        }
    }
    Ok(root)
}

#[derive(Debug, Clone)]
struct RawNode {
    node_id: String, parent_node_id: Option<String>,
    title: String, content: String, level: usize, children: Vec<RawNode>,
    summary: String,
}

fn count_nodes(node: &crate::stores::pageindex_store::PNode) -> usize {
    1 + node.children.iter().map(|c| count_nodes(c)).sum::<usize>()
}

async fn generate_summaries(store: &PageIndexStore, doc_id: &str, root: &RawNode, llm: &OpenAIChat) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !node.content.is_empty() {
            let prompt = format!("用一句话概括以下内容：\n{}", node.content);
            match llm.invoke(vec![Message::human(&prompt)], None).await {
                Ok(resp) => {
                    let summary = resp.content.trim().to_string();
                    let _ = sqlx::query("UPDATE pageindex_nodes SET summary = ? WHERE doc_id = ? AND node_id = ?")
                        .bind(&summary).bind(doc_id).bind(&node.node_id)
                        .execute(&store.pool).await;
                }
                Err(e) => {
                    tracing::warn!("生成摘要失败 (node={}): {}", node.node_id, e);
                }
            }
        }
        for child in &node.children {
            stack.push(child);
        }
    }
}
