# OpenCode 对话存储方案详解

> 本文档基于 OpenCode 真实源码分析其对话历史存储的实现原理。

---

## 目录

1. [概述](#1-概述)
2. [存储架构](#2-存储架构)
3. [表结构设计](#3-表结构设计)
4. [压缩机制原理](#4-压缩机制原理)
5. [溢出检测](#5-溢出检测)
6. [读写流程](#6-读写流程)
7. [同步机制](#7-同步机制)
8. [为什么不用 MongoDB](#8-为什么不用-mongodb)
9. [源码路径](#9-源码路径)
10. [参考资料](#10-参考资料)

---

## 1. 概述

### OpenCode 是什么？

- 100% 开源的 AI 编码助手（MIT 协议）
- 支持多种模型（Claude、OpenAI、本地模型）
- 类似 Claude Code，但 provider-agnostic
- GitHub: https://github.com/anomalyco/opencode

### 核心设计哲学

```
轻量元数据 + JSON 内容
本地 SQLite > 云端 MongoDB
增量压缩 > 全量摘要
事件溯源（支持多端同步）
```

---

## 2. 存储架构

### 架构图

```
┌─────────────────────────────────────────────────────────────┐
│                     OpenCode 存储架构                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ~/.local/share/opencode/                                   │
│  ├── opencode.db           ← SQLite 数据库（主存储）         │
│  │   ├── session           ← 会话元数据                      │
│  │   ├── message           ← 消息内容（JSON）                 │
│  │   ├── part              ← 消息部分（细粒度）               │
│  │   ├── event             ← 同步事件                        │
│  │   └── event_sequence    ← 事件序号                        │
│  │                                                         │
│  ├── storage/              ← JSON 文件（旧方案，已迁移）      │
│  └── sessions/             ← JSONL 文件（可选备份）           │
│                                                             │
│  内存层：                                                    │
│  HashMap<SessionID, RolloutRecorder> ← 活跃会话缓存          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 分层存储

| 层级 | 存储 | 用途 | 特点 |
|------|------|------|------|
| **热数据** | 内存 HashMap | 活跃会话 | 快速读写，实时更新 |
| **温数据** | SQLite（元数据） | 查询列表 | session 表，轻量快速 |
| **冷数据** | SQLite（message） | 按需加载 | JSON 存完整历史 |
| **归档数据** | 可选 JSONL/S3 | 长期保留 | 移动而非删除 |

### 为什么用 SQLite？

```
对话历史的特点：
├── Append-only（追加写入，不修改历史）
├── 按顺序读取（从头到尾）
├── 单用户本地存储（不需要多用户并发）
└── 不需要复杂查询（按 session_id 查）

SQLite 的优势：
├── 本地文件，无需服务器
├── 单进程独占，性能最优
├── WAL 模式，写入不阻塞读取
├── 零成本（比 MongoDB Atlas 省 $62/月）
└── 一个文件包含所有数据
```

---

## 3. 表结构设计

### 3.1 Session 表（元数据）

```typescript
// packages/opencode/src/session/session.sql.ts

export const SessionTable = sqliteTable("session", {
  id: text().$type<SessionID>().primaryKey(),
  project_id: text().$type<ProjectID>().notNull(),
  workspace_id: text().$type<WorkspaceID>(),
  parent_id: text().$type<SessionID>(),        // 支持父子 session（嵌套对话）
  slug: text().notNull(),                      // URL友好的标识
  directory: text().notNull(),                 // 工作目录
  title: text().notNull(),                     // 对话标题
  
  // 版本控制
  version: text().notNull(),
  
  // 统计信息
  summary_additions: integer(),                // 新增代码行数
  summary_deletions: integer(),                // 删除代码行数
  summary_files: integer(),                    // 修改文件数
  summary_diffs: text({ mode: "json" }),       // diff 详情
  
  // 其他
  share_url: text(),                           // 分享链接
  revert: text({ mode: "json" }),              // 回滚信息
  permission: text({ mode: "json" }),          // 权限配置
  
  // 时间戳
  time_created: integer().notNull(),
  time_updated: integer().notNull(),
  time_compacting: integer(),                  // 压缩时间 ⭐
  time_archived: integer(),                    // 归档时间 ⭐
})
```

### 3.2 Message 表（消息内容）

```typescript
// packages/opencode/src/session/session.sql.ts

export const MessageTable = sqliteTable("message", {
  id: text().$type<MessageID>().primaryKey(),
  session_id: text().$type<SessionID>().notNull(),
  
  // 时间戳
  time_created: integer().notNull(),
  time_updated: integer().notNull(),
  
  // JSON 存储完整消息内容 ⭐
  data: text({ mode: "json" }).notNull(),
})
```

**为什么用 JSON 存 data？**

```
消息结构复杂：
{
  role: "assistant",
  parts: [
    { type: "text", content: "我帮你修改了..." },
    { type: "tool", name: "read_file", output: "..." },
    { type: "file", path: "src/main.rs", content: "..." },
    { type: "reasoning", content: "思考过程..." }
  ],
  tokens: { input: 500, output: 200 },
  cost: 0.001,
  model: "claude-3-opus"
}

JSON 的优势：
├── 灵活，不需要拆分成多个字段
├── SQLite 支持 JSON 查询：json_extract(data, '$.role')
├── 序列化/反序列化简单
└── 易于扩展（新增字段不影响 schema）
```

### 3.3 Part 表（消息部分）

```typescript
// packages/opencode/src/session/session.sql.ts

export const PartTable = sqliteTable("part", {
  id: text().$type<PartID>().primaryKey(),
  message_id: text().$type<MessageID>().notNull(),
  session_id: text().$type<SessionID>().notNull(),
  
  // 时间戳
  time_created: integer().notNull(),
  time_updated: integer().notNull(),
  
  // JSON 存储部分内容
  data: text({ mode: "json" }).notNull(),
})
```

**为什么拆分 Part？**

```
一条消息可能有多个 part：
┌─────────────────────────────────────┐
│ Assistant Message                    │
│ ├── TextPart: "我帮你修改了代码"      │
│ ├── ToolPart: read_file              │
│ │   └── output: "很长的文件内容..."   │ ← 可以单独压缩
│ ├── ToolPart: edit_file              │
│ └── FilePart: src/main.rs            │
└─────────────────────────────────────┘

拆分的好处：
├── 压缩时可以单独处理某个 part（剪枝）
├── 大的工具输出可以单独压缩
├── 便于细粒度管理
```

### 3.4 Event 表（同步事件）

```typescript
// packages/opencode/src/sync/event.sql.ts

export const EventSequenceTable = sqliteTable("event_sequence", {
  aggregate_id: text().notNull().primaryKey(),  // 通常是 sessionID
  seq: integer().notNull(),                     // 事件序号
})

export const EventTable = sqliteTable("event", {
  id: text().primaryKey(),
  aggregate_id: text().notNull(),               // sessionID
  seq: integer().notNull(),                     // 序号
  type: text().notNull(),                       // 事件类型
  data: text({ mode: "json" }).notNull(),       // 事件数据
})
```

---

## 4. 压缩机制原理

### 4.1 问题背景

```
对话长了，token 超出模型限制：

GPT-3.5-turbo:  16K tokens
GPT-4:          32K tokens
Claude 3 Opus:  200K tokens

对话历史：
msg1 → msg2 → msg3 → ... → msg100

如果每条消息平均 500 tokens：
100 条 = 50K tokens（可能超出）

问题：
├── 模型拒绝响应（超限）
├── 响应变慢（处理大量历史）
├── 成本增加（按 token 计费）
└── 关键信息丢失（模型"分心"）
```

### 4.2 压缩策略

```
┌─────────────────────────────────────────────────────────────┐
│                   OpenCode 压缩流程                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  步骤 1: 检测溢出                                            │
│  ├── 计算 tokens.input + output + history                   │
│  ├── 对比模型上下文窗口（如 200K）                           │
│  └── 如果超出 → 触发压缩                                     │
│                                                             │
│  步骤 2: 剪枝（Prune）← 第一优先                              │
│  ├── 从后向前扫描消息                                        │
│  ├── 找到旧的工具调用                                        │
│  ├── 压缩工具输出（保留前 2000 字符）                        │
│  ├── 标记 time.compacted = now                              │
│  └── 保留最近 2-5 轮对话完整                                 │
│                                                             │
│  步骤 3: 摘要（Summary）← 最后手段                            │
│  ├── 如果剪枝不够                                            │
│  ├── 生成 Markdown 摘要：                                    │
│  │   ├── Goal（目标）                                        │
│  │   ├── Progress（进度）                                    │
│  │   ├── Key Decisions（关键决定）                           │
│  │   ├── Next Steps（下一步）                                │
│  │   └── Relevant Files（相关文件）                          │
│  ├── 摘要替换旧消息                                          │
│  └── 发送：[摘要] + 最近消息                                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 4.3 源码实现

```typescript
// packages/opencode/src/session/compaction.ts

export const PRUNE_MINIMUM = 20_000     // 最少保留 20K tokens
export const PRUNE_PROTECT = 40_000     // 最多保护 40K tokens
const TOOL_OUTPUT_MAX_CHARS = 2_000     // 工具输出最多 2000 字符
const DEFAULT_TAIL_TURNS = 2            // 保留最近 2 轮对话完整

// 剪枝函数
const prune = Effect.fn("SessionCompaction.prune")(function* (input) {
  const { messages, tokens } = input
  
  // 从后向前遍历
  for (const part of toPrune) {
    if (part.state.status === "completed") {
      // 压缩工具输出
      part.output = part.output.slice(0, TOOL_OUTPUT_MAX_CHARS)
      
      // 标记已压缩
      part.state.time.compacted = Date.now()
      
      // 更新到数据库
      yield* session.updatePart(part)
    }
  }
})

// 摘要模板
const SUMMARY_TEMPLATE = `
## Goal
[描述用户的目标]

## Constraints & Preferences
[用户的限制和偏好]

## Progress (Done/In Progress/Blocked)
[当前进度]

## Key Decisions
[做出的关键决定]

## Next Steps
[下一步行动]

## Critical Context
[关键上下文信息]

## Relevant Files
[相关文件列表]
---
`
```

### 4.4 剪枝策略详解

```
为什么从后向前？

消息顺序：
[msg1, msg2, msg3, ..., msg50]

用户当前问题是 msg50
模型需要最近几条消息理解上下文
旧消息可能不重要

剪枝流程：
┌────────────────────────────────────┐
│ 消息队列（从后向前扫描）            │
│                                    │
│ msg50 ← 最新（保留完整）            │
│ msg49 ← 保留完整                    │
│ msg48 ← 保留完整                    │
│ msg47 ← 检查是否有工具调用          │
│   ├── 有：压缩 output               │
│   ├── 无：保留                      │
│ msg46 ← 检查...                     │
│ ...                                 │
│ msg1 ← 最旧（可能被摘要）            │
└────────────────────────────────────┘

保留策略：
├── 最近 2-5 轮对话完整（DEFAULT_TAIL_TURNS）
├── 用户设定（如"我的名字是Bob"）不压缩
├── 错误消息完整保留（用于调试）
└── 工具输出压缩到 2000 字符
```

---

## 5. 溢出检测

### 5.1 Token 计算

```typescript
// packages/opencode/src/session/overflow.ts

export function isOverflow(input: {
  tokens: {
    input: number,      // 用户输入
    output: number,     // 模型输出
    system: number,     // 系统提示
    tools: number,      // 工具定义
    history: number,    // 历史消息
  },
  model: {
    context_window: number,  // 如 200K
  }
}) {
  // 计算总 tokens
  const count = tokens.input 
    + tokens.output 
    + tokens.system 
    + tokens.tools 
    + tokens.history
  
  // 可用空间（留 buffer）
  const buffer = 10_000  // 留 10K 给新输出
  const usable = model.context_window - buffer
  
  // 判断是否溢出
  return count >= usable
}

// 使用示例
const overflow = isOverflow({
  tokens: {
    input: 500,
    output: 3000,
    system: 2000,
    tools: 1000,
    history: 180000,  // 180K 历史消息
  },
  model: {
    context_window: 200000,  // Claude 200K
  }
})

// overflow = true（触发压缩）
```

### 5.2 估算方法

```typescript
// 简化估算（不精确但快）

function estimateTokens(text: string): number {
  const charCount = text.length
  
  // 统计中文字符
  const chineseChars = text
    .split('')
    .filter(c => c > '\u{4E00}' && c < '\u{9FFF}')
    .length
  
  const otherChars = charCount - chineseChars
  
  // 中文约 2 tokens/字，英文约 0.25 tokens/字（4 chars ≈ 1 token）
  return (chineseChars * 2 + otherChars) / 4 + 1
}

// 精确计算（使用 tiktoken）
import tiktoken from 'tiktoken'

function countTokens(text: string, model: string): number {
  const encoding = tiktoken.encoding_for_model(model)
  return len(encoding.encode(text))
}
```

---

## 6. 读写流程

### 6.1 写入流程

```
用户发送消息 → 模型回复：

┌─────────────────────────────────────────────────────────────┐
│ 步骤 1: 写入 message 表                                      │
│ INSERT INTO message                                          │
│ (id, session_id, data, time_created)                         │
│ VALUES                                                       │
│ ("msg001", "abc123", '{"role":"user",...}', 1714272000)      │
│                                                             │
│ 步骤 2: 更新 session 表                                      │
│ UPDATE session                                               │
│ SET time_updated = 1714272000                                │
│ WHERE id = "abc123"                                          │
│                                                             │
│ 步骤 3: 写入内存缓存                                         │
│ active_threads["abc123"].push(message)                       │
│                                                             │
│ 步骤 4: 检测溢出（异步）                                      │
│ if (isOverflow(session)) {                                   │
│   runCompaction(session)                                     │
│ }                                                            │
│                                                             │
│ 步骤 5: 返回响应给用户                                       │
│ （压缩不阻塞响应）                                            │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 读取流程

```
用户打开历史对话：

┌─────────────────────────────────────────────────────────────┐
│ 步骤 1: 查 session 表（获取元数据）                          │
│ SELECT id, title, time_updated                               │
│ FROM session                                                 │
│ WHERE id = "abc123"                                          │
│                                                             │
│ 步骤 2: 查 message 表（获取消息）                            │
│ SELECT id, data, time_created                                │
│ FROM message                                                 │
│ WHERE session_id = "abc123"                                  │
│ ORDER BY time_created                                        │
│                                                             │
│ 步骤 3: 解析 JSON                                            │
│ for (msg of messages) {                                      │
│   const data = JSON.parse(msg.data)                         │
│   // {role: "user", parts: [...]}                            │
│ }                                                            │
│                                                             │
│ 步骤 4: 检测溢出                                             │
│ if (isOverflow(messages)) {                                  │
│   // 触发压缩（首次打开时可能压缩）                          │
│ }                                                            │
│                                                             │
│ 步骤 5: 返回给前端                                           │
│ messages.slice(-50)  // 只返回最近 50 条                     │
└─────────────────────────────────────────────────────────────┘
```

### 6.3 查询列表流程

```
用户查看对话列表（侧边栏）：

┌─────────────────────────────────────────────────────────────┐
│ 只查 session 表（不加载消息内容）                            │
│                                                             │
│ SELECT id, title, time_updated, summary_files               │
│ FROM session                                                 │
│ WHERE user_id = "user001"                                    │
│ ORDER BY time_updated DESC                                   │
│ LIMIT 50                                                     │
│                                                             │
│ 返回：                                                       │
│ [                                                            │
│   {id: "abc1", title: "Python开发", time: "3小时前"},       │
│   {id: "abc2", title: "翻译文档", time: "昨天"},             │
│   ...                                                        │
│ ]                                                            │
│                                                             │
│ 为什么不加载消息？                                           │
│ ├── 列表只需要元数据（标题、时间）                           │
│ ├── 加载消息内容很慢（JSON 大）                              │
│ ├── 用户点击才加载完整内容                                   │
│ └── 类似 ChatGPT/Claude Web 的设计                           │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. 同步机制

### 7.1 事件溯源（Event Sourcing）

```
传统方案：
直接修改 session 表
UPDATE session SET title = "新标题" WHERE id = "abc123"

问题：
├── 无法回滚（改了就改了）
├── 无法追踪历史（谁改了什么）
├── 多端同步困难（冲突处理）

OpenCode 方案：
事件溯源（Event Sourcing）
├── 不直接修改 session
├── 写入 event 表
├── 从 event 重建状态
└── 支持回滚、回放、同步
```

### 7.2 Event 结构

```typescript
// packages/opencode/src/session/session.ts

export const Event = {
  Created: SyncEvent.define({ 
    type: "session.created", 
    version: 1 
  }),
  Updated: SyncEvent.define({ 
    type: "session.updated", 
    version: 1 
  }),
  Deleted: SyncEvent.define({ 
    type: "session.deleted", 
    version: 1 
  }),
}

// 事件示例
{
  aggregate_id: "abc123",    // sessionID
  seq: 10,                   // 序号（递增）
  type: "session.updated",
  data: {
    title: "新标题",
    updated_by: "user001",
    timestamp: 1714272000
  }
}
```

### 7.3 同步流程

```
多端同步（电脑 + 手机）：

┌─────────────────────────────────────────────────────────────┐
│ 电脑端修改标题                                              │
│                                                             │
│ 步骤 1: 写入 event                                          │
│ INSERT INTO event                                           │
│ (aggregate_id, seq, type, data)                             │
│ VALUES                                                      │
│ ("abc123", 10, "session.updated", '{"title":"新标题"}')     │
│                                                             │
│ 步骤 2: 更新 event_sequence                                 │
│ UPDATE event_sequence                                       │
│ SET seq = 10                                                │
│ WHERE aggregate_id = "abc123"                               │
│                                                             │
│ 步骤 3: 推送到云端（可选）                                   │
│ POST /api/sync                                              │
│ {events: [event]}                                           │
│                                                             │
│ 手机端同步：                                                │
│                                                             │
│ 步骤 1: 拉取云端事件                                        │
│ GET /api/sync?since_seq=5                                   │
│                                                             │
│ 步骤 2: 写入本地 event 表                                   │
│ INSERT INTO event ...                                       │
│                                                             │
│ 步骤 3: 重建状态                                            │
│ SELECT * FROM event                                         │
│ WHERE aggregate_id = "abc123"                               │
│ ORDER BY seq                                                │
│                                                             │
│ 步骤 4: 应用事件                                            │
│ for (event of events) {                                     │
│   apply(event)  // 更新 session                             │
│ }                                                            │
└─────────────────────────────────────────────────────────────┘
```

---

## 8. 为什么不用 MongoDB？

### 8.1 MongoDB 设计用途

```
MongoDB 设计用于：
├── 多用户并发（分布式）
├── 复杂查询（聚合、筛选）
├── 随机更新（部分字段）
└── 大数据分析

对话历史的特点：
├── 单用户（本地 CLI）
├── 简单查询（按 session_id）
├── Append-only（追加，不更新）
└── 不需要分析
```

### 8.2 成本对比

```
存储 100GB 对话历史：

MongoDB Atlas:
├── M10 集群: $62.5/月
├── 需要 VPN/网络连接
├── 延迟 50-100ms
└── 不必要的复杂查询能力

SQLite:
├── 本地文件: $0
├── 无需网络
├── 延迟 1-5ms
└── 完美匹配对话历史特点

AWS S3（归档）:
├── $2.3/月（100GB）
├── 适合冷数据
└── OpenCode 可选方案
```

### 8.3 性能对比

```
MongoDB 单表存 5000 万条消息：

查询一个会话：
find({session_id: "xxx"}).sort({timestamp: 1})
├── 需扫描索引
├── 延迟 10-50ms
├── 内存占用高（索引）
└── 成本高（集群）

SQLite + JSONL：
直接读取该会话文件
├── 无需索引
├── 延迟 1-5ms
├── 内存占用低
└── 成本 $0
```

---

## 9. 源码路径

### 核心文件

```
packages/opencode/src/
├── session/                  ← 对话核心逻辑
│   ├── session.ts           ← Session 服务实现
│   ├── session.sql.ts       ← SQLite Schema (Drizzle ORM) ⭐
│   ├── message.ts           ← Message 数据结构
│   ├── message-v2.ts        ← 新版 Message 实现
│   ├── compaction.ts        ← 压缩/摘要机制 ⭐
│   ├── overflow.ts          ← 上下文溢出检测
│   └── schema.ts            ← ID 类型定义
│
├── storage/                  ← 存储层
│   ├── db.ts                ← SQLite 数据库连接 ⭐
│   ├── db.bun.ts            ← Bun 运行时适配
│   ├── db.node.ts           ← Node.js 运行时适配
│   ├── storage.ts           ← 文件存储（JSON，旧方案）
│   ├── schema.sql.ts        ← 通用 Schema
│   └── json-migration.ts    ← JSON→SQLite 迁移
│
├── sync/                     ← 事件同步系统
│   ├── index.ts             ← SyncEvent 核心
│   ├── event.sql.ts         ← Event Schema
│   └── README.md            ← 同步架构文档
```

### 关键代码片段位置

```
表结构定义：packages/opencode/src/session/session.sql.ts
压缩逻辑：packages/opencode/src/session/compaction.ts
溢出检测：packages/opencode/src/session/overflow.ts
数据库连接：packages/opencode/src/storage/db.ts
事件同步：packages/opencode/src/sync/index.ts
```

---

## 10. 参考资料

- [OpenCode GitHub](https://github.com/anomalyco/opencode)
- [OpenCode 官方文档](https://opencode.ai/docs)
- [Drizzle ORM](https://orm.drizzle.team/)
- [SQLite WAL 模式](https://www.sqlite.org/wal.html)
- [Event Sourcing](https://martinfowler.com/eaaDev/EventSourcing.html)

---

*文档整理时间：2026-04-27*
*基于 OpenCode 真实源码分析*
*用途：学习与参考*