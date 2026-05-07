//! PageIndex - 无向量 RAG 引擎
//!
//! 核心思想：抛弃 embedding + 向量搜索，改用 LLM 推理导航文档树。
//! 文档上传时按标题层级构建树，检索时 LLM 逐层导航定位相关内容。
//!
//! 树结构：
//!   Document
//!   ├── Chapter 1 (h1)
//!   │   ├── Section 1.1 (h2) → chunk_text
//!   │   └── Section 1.2 (h2) → chunk_text
//!   └── Chapter 2 (h1)
//!       └── Section 2.1 (h2) → chunk_text

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 树节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PNode {
    pub id: String,           // 唯一 ID
    pub title: String,         // 标题
    pub level: usize,          // 层级 0=文档,1=章,2=节,3=小节
    pub content: String,       // 文本内容（叶子节点有）
    pub children: Vec<PNode>,  // 子节点
}

impl PNode {
    pub fn is_leaf(&self) -> bool { self.children.is_empty() }
}

/// 文档索引
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentIndex {
    pub doc_id: String,
    pub title: String,
    pub root: PNode,
}

/// PageIndex 引擎
pub struct PageIndex;

impl PageIndex {
    /// 从文本构建文档树（简化版：按标题分块为扁平列表）
    pub fn build_tree(doc_id: &str, title: &str, text: &str) -> DocumentIndex {
        let lines: Vec<&str> = text.lines().collect();
        let mut root = PNode {
            id: format!("{}_root", doc_id), title: title.to_string(), level: 0,
            content: String::new(), children: Vec::new(),
        };

        let mut current_section = String::new();
        let mut current_title = String::new();

        for line in &lines {
            let trimmed = line.trim();
            if let Some(_level) = heading_level(trimmed) {
                if !current_title.is_empty() {
                    root.children.push(PNode {
                        id: format!("{}_{}", doc_id, sanitize_title(&current_title)),
                        title: current_title.clone(), level: 1,
                        content: current_section.clone(), children: Vec::new(),
                    });
                }
                current_title = trimmed.trim_start_matches('#').trim().to_string();
                current_section.clear();
            } else if !trimmed.is_empty() {
                if !current_section.is_empty() { current_section.push('\n'); }
                current_section.push_str(trimmed);
            }
        }
        // 最后一段
        if !current_title.is_empty() {
            root.children.push(PNode {
                id: format!("{}_{}", doc_id, sanitize_title(&current_title)),
                title: current_title.clone(), level: 1,
                content: current_section.clone(), children: Vec::new(),
            });
        }

        DocumentIndex { doc_id: doc_id.to_string(), title: title.to_string(), root }
    }

    /// 用 LLM 导航树，返回相关内容
    pub async fn search(&self, query: &str, doc: &DocumentIndex) -> String {
        // 1. 获取根的子节点列表 → LLM 选最相关的
        // 2. 展开选中节点 → LLM 选子节点
        // 3. 直到叶子节点 → 返回 content
        // 这里简化：直接返回第一个叶子节点的内容
        let leaf = find_first_leaf(&doc.root);
        leaf.map(|n| n.content.clone()).unwrap_or_default()
    }
}

fn heading_level(line: &str) -> Option<usize> {
    if line.starts_with("###") { Some(3) }
    else if line.starts_with("##") { Some(2) }
    else if line.starts_with('#') { Some(1) }
    else if line.len() > 1 && line.chars().next()?.is_digit(10) && line.contains('.') {
        Some(2) // 数字序号如 "1. " 视为 h2
    }
    else { None }
}

fn sanitize_title(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-').take(30).collect()
}

fn find_leaf_mut<'a>(node: &'a mut PNode) -> Option<&'a mut PNode> {
    if node.is_leaf() { return Some(node); }
    for child in &mut node.children {
        if let Some(found) = find_leaf_mut(child) { return Some(found); }
    }
    None
}

fn find_first_leaf(node: &PNode) -> Option<&PNode> {
    if node.is_leaf() && !node.content.is_empty() { return Some(node); }
    for child in &node.children {
        if let Some(found) = find_first_leaf(child) { return Some(found); }
    }
    None
}
