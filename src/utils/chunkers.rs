//! 自定义文本分割器
//!
//! 提供 langchainrust crate 未内置的切分策略：
//! - TokenTextSplitter: 按 token 数切分（估计算法，无需外部依赖）
//! - SemanticChunker: 用 Embedding 检测话题边界

use crate::errors::ProcessError;
use langchainrust::{Document, TextSplitter, Embeddings, OpenAIEmbeddings};
use std::sync::Arc;

/// Token 文本分割器
/// 按 token 数（估算）切分，精确控制上下文窗口
pub struct TokenTextSplitter {
    chunk_tokens: usize,
    overlap_tokens: usize,
}

impl TokenTextSplitter {
    pub fn new(chunk_tokens: usize, overlap_tokens: usize) -> Self {
        Self { chunk_tokens, overlap_tokens }
    }
}

impl TextSplitter for TokenTextSplitter {
    fn split_text(&self, text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        if text.is_empty() {
            return chunks;
        }

        let chars: Vec<char> = text.chars().collect();
        let total = chars.len();
        let mut start = 0;

        while start < total {
            let target_end = self.find_split_end(&chars, start);
            let chunk: String = chars[start..target_end].iter().collect();
            chunks.push(chunk);
            start = target_end;
        }

        if self.overlap_tokens > 0 && chunks.len() > 1 {
            let overlap_chars = self.overlap_tokens * 2;
            let mut overlapped = Vec::new();
            for (i, chunk) in chunks.into_iter().enumerate() {
                if i == 0 {
                    overlapped.push(chunk);
                } else {
                    let prev = &overlapped[i - 1];
                    let pchars: Vec<char> = prev.chars().collect();
                    let start_overlap = pchars.len().saturating_sub(overlap_chars);
                    let overlap: String = pchars[start_overlap..].iter().collect();
                    overlapped.push(format!("{}{}", overlap, chunk));
                }
            }
            chunks = overlapped;
        }

        chunks
    }
}

impl TokenTextSplitter {
    fn find_split_end(&self, chars: &[char], start: usize) -> usize {
        let max_chars = self.chunk_tokens * 2;

        if start >= chars.len() {
            return chars.len();
        }

        let end = (start + max_chars).min(chars.len());
        if end == chars.len() {
            return end;
        }

        let search_start = (end.saturating_sub(max_chars / 4)).max(start + 1);
        for i in (search_start..end).rev() {
            let c = chars[i];
            if c == '\n' && i + 1 < chars.len() && chars[i + 1] == '\n' {
                return (i + 2).min(chars.len());
            }
        }
        for i in (search_start..end).rev() {
            if chars[i] == '\n' {
                return (i + 1).min(chars.len());
            }
        }
        for i in (search_start..end).rev() {
            if chars[i] == '。' || chars[i] == '！' || chars[i] == '？' {
                return (i + 1).min(chars.len());
            }
        }
        for i in (search_start..end).rev() {
            if chars[i] == '.' || chars[i] == '!' || chars[i] == '?' {
                return (i + 1).min(chars.len());
            }
        }
        for i in (search_start..end).rev() {
            if chars[i] == '；' || chars[i] == '，' || chars[i] == ' ' || chars[i] == ',' {
                return (i + 1).min(chars.len());
            }
        }

        end
    }
}

/// 语义分割器：用 Embedding 检测话题边界
pub struct SemanticChunker {
    embeddings: Arc<OpenAIEmbeddings>,
    min_chunk_chars: usize,
    max_chunk_chars: usize,
}

impl SemanticChunker {
    pub fn new(embeddings: Arc<OpenAIEmbeddings>, min_chunk_chars: usize, max_chunk_chars: usize) -> Self {
        Self { embeddings, min_chunk_chars, max_chunk_chars }
    }

    pub async fn split_document_semantic(&self, document: &Document) -> Result<Vec<Document>, ProcessError> {
        let sentences = split_sentences(&document.content);
        if sentences.len() <= 1 {
            return Ok(vec![document.clone()]);
        }

        // 分批调 Embedding API（单次最多25条）
        let mut vectors = Vec::new();
        for chunk in sentences.chunks(20) {
            let refs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            let batch = self.embeddings.embed_documents(&refs).await
                .map_err(|e| ProcessError::LoadError(format!("Embedding 失败: {}", e)))?;
            vectors.extend(batch);
        }

        if vectors.is_empty() || vectors.len() <= 1 {
            return Ok(vec![document.clone()]);
        }

        // 计算相邻相似度，找话题边界
        let mut boundaries = vec![0usize];
        let mut current_chars = 0usize;

        for i in 1..sentences.len() {
            let sim = cosine_similarity(&vectors[i - 1], &vectors[i]);
            let s_len = sentences[i].chars().count();

            if sim < 0.45 && current_chars >= self.min_chunk_chars {
                boundaries.push(i);
                current_chars = 0;
            } else if current_chars + s_len > self.max_chunk_chars {
                boundaries.push(i);
                current_chars = 0;
            } else {
                current_chars += s_len;
            }
        }
        boundaries.push(sentences.len());

        // 构建 chunks
        let mut chunks = Vec::new();
        for i in 0..boundaries.len() - 1 {
            let start = boundaries[i];
            let end = boundaries[i + 1];
            let text: String = sentences[start..end].join("");
            if text.trim().is_empty() { continue; }
            let mut meta = document.metadata.clone();
            meta.insert("chunk_strategy".to_string(), "semantic".to_string());
            chunks.push(Document { content: text, metadata: meta, id: None });
        }

        if chunks.is_empty() {
            Ok(vec![document.clone()])
        } else {
            Ok(chunks)
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

fn split_sentences(text: &str) -> Vec<String> {
    let separators = ['。', '！', '？', '!', '?', '\n'];
    let mut result = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if separators.contains(&c) {
            let s = cur.trim().to_string();
            if !s.is_empty() { result.push(s); }
            cur.clear();
        }
    }
    let rem = cur.trim().to_string();
    if !rem.is_empty() { result.push(rem); }
    result
}

/// 估算 token 数（中文 1 char ≈ 1 token，英文 4 chars ≈ 1 token）
pub fn estimate_tokens(text: &str) -> usize {
    let mut chinese_chars = 0;
    let mut ascii_chars = 0;
    for c in text.chars() {
        if c as u32 > 0x7F {
            chinese_chars += 1;
        } else if !c.is_whitespace() {
            ascii_chars += 1;
        }
    }
    chinese_chars + ascii_chars / 4 + 1
}
