//! 自定义文本分割器
//!
//! 提供 langchainrust crate 未内置的切分策略：
//! - TokenTextSplitter: 按 token 数切分（估计算法，无需外部依赖）

use langchainrust::TextSplitter;

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
