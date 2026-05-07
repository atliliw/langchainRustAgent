use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PNode {
    pub id: String,
    pub title: String,
    pub level: usize,
    pub content: String,
    pub children: Vec<PNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentIndex {
    pub doc_id: String,
    pub title: String,
    pub root: PNode,
}

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

static STORE: Mutex<Option<HashMap<String, DocumentIndex>>> = Mutex::new(None);
fn store() -> &'static Mutex<Option<HashMap<String, DocumentIndex>>> { &STORE }

pub struct PageIndex;

impl PageIndex {
    pub fn build_and_store(req: &BuildRequest) -> Result<DocumentIndex, String> {
        let idx = Self::build_tree(&req.doc_id, &req.title, &req.text);
        store().lock().map_err(|e| e.to_string())?
            .get_or_insert_with(HashMap::new)
            .insert(req.doc_id.clone(), idx.clone());
        Ok(idx)
    }

    pub async fn search(config: &Config, req: &SearchRequest) -> Result<SearchResponse, String> {
        let doc = store().lock().map_err(|e| e.to_string())?
            .as_ref().and_then(|m| m.get(&req.doc_id)).cloned()
            .ok_or_else(|| "文档不存在".to_string())?;

        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(512));
        let mut path = vec!["根".to_string()];
        let mut current_nodes = doc.root.children.clone();
        let mut result = String::new();

        for _ in 0..3 {
            if current_nodes.is_empty() { break; }
            let list: String = current_nodes.iter().enumerate()
                .map(|(i,n)| format!("{}. {}: {}", i+1, n.title, n.content.chars().take(80).collect::<String>()))
                .collect::<Vec<_>>().join("\n");
            let p = format!("根据查询选择最相关的（只返回序号）：\n\n查询：{}\n\n{}", req.query, list);
            let resp = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| e.to_string())?;
            let idx = resp.content.trim().parse::<usize>().unwrap_or(1).saturating_sub(1).min(current_nodes.len()-1);
            path.push(current_nodes[idx].title.clone());
            if !current_nodes[idx].content.is_empty() { result = current_nodes[idx].content.clone(); }
            current_nodes = current_nodes[idx].children.clone();
        }
        if result.is_empty() {
            result = doc.root.children.iter().map(|n| n.content.clone()).collect::<Vec<_>>().join("\n\n");
        }
        Ok(SearchResponse { query: req.query.clone(), result: result.chars().take(500).collect(), path })
    }

    fn build_tree(doc_id: &str, title: &str, text: &str) -> DocumentIndex {
        let lines: Vec<&str> = text.lines().collect();
        let mut root = PNode { id: format!("{}_root",doc_id), title: title.to_string(), level: 0, content: String::new(), children: vec![] };
        let mut ct = String::new();
        let mut cs = String::new();
        for line in &lines {
            let t = line.trim();
            if t.starts_with('#') {
                if !ct.is_empty() { root.children.push(PNode { id: format!("{}_{}",doc_id,ct.replace(" ", "_")), title: ct.clone(), level: 1, content: cs.clone(), children: vec![] }); }
                ct = t.trim_start_matches('#').trim().to_string();
                cs.clear();
            } else if !t.is_empty() { if !cs.is_empty() { cs.push('\n'); } cs.push_str(t); }
        }
        if !ct.is_empty() { root.children.push(PNode { id: format!("{}_{}",doc_id,ct.replace(" ", "_")), title: ct, level: 1, content: cs, children: vec![] }); }
        DocumentIndex { doc_id: doc_id.to_string(), title: title.to_string(), root }
    }
}
