# Function Calling 机制说明

## 一句话

**Function Calling 是让 LLM 按你规定的 JSON 格式输出，而不是自由发挥文本。**

---

## 为什么需要 FC？

### 不用 FC（旧方式）

```
prompt: "输出决策结果（充分/不充分）"
LLM:   "满足"  ← 不听话，需要 keyword 匹配去猜
```

### 用 FC

```
prompt: "判断信息是否足够"
tools: [{"enum": ["充分", "不充分"]}]
LLM:   {"route": "充分"}  ← 铁定格式正确，不用猜
```

---

## 核心概念：tools 参数

`tools` 是请求体里的一个数组，**定义了 LLM 可以使用的"答题卡"**。

### 没有 tools 时

```json
{
  "model": "qwen-turbo",
  "messages": [
    {"role": "user", "content": "输出充分或不充分"}
  ]
}
```

LLM 返回自由文本，你无法控制格式：

```json
{
  "content": "满足"
}
```

### 有 tools 时

```json
{
  "model": "qwen-turbo",
  "messages": [
    {"role": "user", "content": "判断信息是否足够"}
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "make_decision",
        "description": "输出决策结果",
        "parameters": { ... }
      }
    }
  ],
  "tool_choice": "required"
}
```

LLM **不能返回文本**，必须通过 `tool_calls` 返回结构化数据：

```json
{
  "content": null,
  "tool_calls": [
    {
      "id": "call_abc123",
      "type": "function",
      "function": {
        "name": "make_decision",
        "arguments": "{\"route\": \"充分\"}"
      }
    }
  ]
}
```

---

## tools 完整字段详解

### 外层结构

```json
{
  "name": "create_task_plan",         // ← 函数名，LLM 用这个来引用
  "description": "将用户任务拆解为子任务列表",  // ← 描述，告诉 LLM 这个函数是干什么的
  "parameters": {                      // ← JSON Schema，定义参数格式
    ...                                //     LLM 必须按这个 schema 输出
  }
}
```

### parameters

`parameters` 是一个 [JSON Schema](https://json-schema.org/)，定义 LLM 必须输出的数据格式。LLM 会严格按照这个 schema 生成 `arguments`。

---

## 项目中的 tool 定义（一）：create_task_plan

**位置**：`src/services/agent_executor.rs` 规划阶段

**作用**：让 LLM 输出结构化的子任务列表

### 定义

```json
{
  "name": "create_task_plan",
  "description": "将用户任务拆解为子任务列表",

  "parameters": {
    "type": "object",              // 参数整体是一个 JSON 对象
                                   // 固定为 "object"

    "properties": {                // 这个对象包含的字段

      "tasks": {                   // 唯一的顶级字段
                                   // LLM 需要在这里输出所有子任务

        "type": "array",           // tasks 是一个数组

        "items": {                 // 数组里每个元素（每个子任务）的结构
          "type": "object",        // 每个子任务也是一个 JSON 对象

          "properties": {          // 子任务对象的字段

            "name": {              // 子任务名
              "type": "string",
              "description": "子任务名（中文）"
            },

            "description": {       // 子任务描述
              "type": "string",
              "description": "做什么"
            },

            "tool": {              // 分配的工具
              "type": "string",
              "description": "工具名（rag_search / web_search / llm_query 等）"
            },

            "task_type": {         // 任务类型
              "type": "string",
              "enum": [            // ← LLM 只能从这三个里选
                "normal",          //   普通任务，调工具执行
                "decision",        //   决策节点，需要 LLM 判断后选路径
                "human_review"     //   人工审批节点，等待用户确认
              ]
            },

            "depends_on": {        // 依赖的前置任务
              "type": "array",     // 数组
              "items": {           // 元素
                "type": "string"   // 前置任务的名字
              }
            },

            "input_template": {    // 输入模板
              "type": "string"     // 给工具传的参数模板
            },

            "routes": {            // 仅 decision 类型需要
              "type": "object",
              "description": "决策节点的路由表",
              "additionalProperties": {   // key-value 对
                "type": "array",          // value 是任务名数组
                "items": {
                  "type": "string"
                }
              }
            }
          },

          "required": [            // 这三个字段必填
            "name",                // 子任务名
            "tool",                // 分配的工具
            "depends_on"           // 依赖项（空数组表示没有依赖）
          ]
        }
      }
    },

    "required": ["tasks"]          // 顶级必须包含 tasks 字段
  }
}
```

### LLM 返回示例

```json
{
  "content": null,
  "tool_calls": [
    {
      "id": "call_abc123",
      "type": "function",
      "function": {
        "name": "create_task_plan",
        "arguments": "{\"tasks\":[{\"name\":\"搜索Go核心特性\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\"},{\"name\":\"搜索Python核心特性\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\"},{\"name\":\"判断信息是否充分\",\"tool\":\"\",\"depends_on\":[\"搜索Go核心特性\",\"搜索Python核心特性\"],\"task_type\":\"decision\",\"routes\":{\"充分\":[],\"不充分\":[\"补充搜索Go\",\"补充搜索Python\"]}},{\"name\":\"补充搜索Go\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\"},{\"name\":\"补充搜索Python\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\"},{\"name\":\"写对比总结\",\"tool\":\"llm_query\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\"}]}"
      }
    }
  ]
}
```

`arguments` 是 JSON 字符串，解析后：

```json
{
  "tasks": [
    {
      "name": "搜索Go核心特性",
      "tool": "rag_search",
      "depends_on": [],
      "task_type": "normal"
    },
    {
      "name": "搜索Python核心特性",
      "tool": "rag_search",
      "depends_on": [],
      "task_type": "normal"
    },
    {
      "name": "判断信息是否充分",
      "tool": "",
      "depends_on": ["搜索Go核心特性", "搜索Python核心特性"],
      "task_type": "decision",
      "routes": {
        "充分": [],
        "不充分": ["补充搜索Go", "补充搜索Python"]
      }
    },
    {
      "name": "补充搜索Go",
      "tool": "web_search",
      "depends_on": ["判断信息是否充分"],
      "task_type": "normal"
    },
    {
      "name": "补充搜索Python",
      "tool": "web_search",
      "depends_on": ["判断信息是否充分"],
      "task_type": "normal"
    },
    {
      "name": "写对比总结",
      "tool": "llm_query",
      "depends_on": ["判断信息是否充分"],
      "task_type": "normal"
    }
  ]
}
```

### 代码解析

```rust
// r.tool_calls         → Option<Vec<ToolCall>>
//   .as_ref()          → 取引用
//   .and_then(|calls| calls.first())  → 取第一个 tool_call
//   .and_then(|call| {
//     let args: Value = serde_json::from_str(call.arguments()).ok()?;
//     // call.arguments() 返回的是 JSON 字符串
//     // 需要用 from_str 先解析为 Value
//     serde_json::from_value(args["tasks"].clone()).ok()
//     // 从 Value 中取 tasks 字段，反序列化为 Vec<AgentTask>
//   })
//   .unwrap_or_default()

let mut tasks: Vec<AgentTask> = r.tool_calls.as_ref()
    .and_then(|calls| calls.first())
    .and_then(|call| {
        let args: serde_json::Value = serde_json::from_str(call.arguments()).ok()?;
        serde_json::from_value(args["tasks"].clone()).ok()
    })
    .unwrap_or_default();
```

---

## 项目中的 tool 定义（二）：make_decision

**位置**：`src/services/agent_executor.rs` 决策节点执行

**作用**：约束 LLM 只能输出 `"充分"` 或 `"不充分"`

### 定义

```json
{
  "name": "make_decision",
  "description": "输出决策结果",

  "parameters": {
    "type": "object",              // 参数是对象

    "properties": {

      "route": {                   // 路由结果
        "type": "string",
        "enum": [                  // ← LLM 只能二选一
          "充分",                   //   信息足够，跳过补充搜索
          "不充分"                   //   信息不够，需要补充搜索
        ]
      },

      "reason": {                  // 决策理由
        "type": "string",
        "description": "决策理由"    // LLM 自由发挥，写一段文本解释为什么这么选
      }

    },

    "required": [                   // 这两个字段都要填
      "route",                      // route 必须有值
      "reason"                      // reason 必须有值
    ]
  }
}
```

### LLM 返回示例

```json
{
  "content": null,
  "tool_calls": [
    {
      "id": "call_def456",
      "type": "function",
      "function": {
        "name": "make_decision",
        "arguments": "{\"route\":\"充分\",\"reason\":\"搜索到的信息已经覆盖了RAG的核心概念和流程\"}"
      }
    }
  ]
}
```

解析后：

```json
{
  "route": "充分",
  "reason": "搜索到的信息已经覆盖了RAG的核心概念和流程"
}
```

### 代码解析

```rust
// r.tool_calls                   → Option<Vec<ToolCall>>
//   .as_ref()                    → 取引用
//   .and_then(|calls| calls.first())  → 取第一个
//   .and_then(|call|
//     serde_json::from_str::<Value>(call.arguments()).ok()
//     // arguments 是 JSON 字符串，解析为 Value
//   )
//   .and_then(|args| args["route"].as_str().map(|s| s.to_string()))
//   // 从 Value 中取 route 字段
//   .unwrap_or_else(|| "充分")

let route = r.tool_calls.as_ref()
    .and_then(|calls| calls.first())
    .and_then(|call| serde_json::from_str::<serde_json::Value>(call.arguments()).ok())
    .and_then(|args| args["route"].as_str().map(|s| s.to_string()))
    .unwrap_or_else(|| "充分".to_string());
```

---

## 这两个是"真工具"吗？

**不是。** 它们是"伪工具"——借 FC 的壳来约束 LLM 输出格式。

| | 真工具（ToolCallingEngine） | 伪工具（plan/decision） |
|---|---|---|
| 例子 | `rag_search`、`web_search` | `create_task_plan`、`make_decision` |
| LLM 返回后 | 代码执行工具，结果返回 LLM | 直接解析参数取值，不执行任何东西 |
| 后续 | LLM 继续推理 | 结束 |
| `tool_choice` | `"auto"` | `"required"` |

**真工具流程**：

```
LLM 返回 tool_calls("rag_search", {"query":"RAG"})
  → 执行 rag_search（调向量库）
  → 结果拼回 messages
  → 发给 LLM 继续
```

**伪工具流程**：

```
LLM 返回 tool_calls("make_decision", {"route":"充分"})
  → 解析 route = "充分"
  → 结束，不调任何函数
```

---

## tool_choice 说明

| 值 | 效果 | 项目中使用位置 |
|---|---|---|
| `"auto"` | LLM 自己决定调不调工具 | `ToolCallingEngine::chat()` |
| `"required"` | LLM 必须调一个工具 | `plan()`、决策节点 |
| `"none"` | 禁止调工具 | 未使用 |
| `{"type":"function","function":{"name":"xxx"}}` | 强制调指定工具 | 未使用 |

---

## 完整请求体示例（规划阶段）

```json
{
  "model": "qwen-turbo",
  "messages": [
    {
      "role": "user",
      "content": "将用户任务拆解为子任务并分配工具。\n可用工具：[{\"name\":\"rag_search\",\"description\":\"检索知识库\"},...]\n任务：比较Go和Python\n通过 create_task_plan 函数输出规划结果。"
    }
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "create_task_plan",
        "description": "将用户任务拆解为子任务列表",
        "parameters": {
          "type": "object",
          "properties": {
            "tasks": {
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "name": {"type": "string"},
                  "tool": {"type": "string"},
                  "task_type": {"enum": ["normal", "decision", "human_review"]},
                  "depends_on": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["name", "tool", "depends_on"]
              }
            }
          },
          "required": ["tasks"]
        }
      }
    }
  ],
  "tool_choice": "required"
}
```

---

## 解析流程总结

```
LLM 返回
  ↓
LLMResult {
  content: null,               ← 没文本
  tool_calls: Some([           ← 有函数调用
    ToolCall {
      id: "call_xxx",
      function: FunctionCall {
        name: "函数名",          ← 区分调了哪个
        arguments: "{\"key\":\"value\"}"  ← 参数是 JSON 字符串
      }
    }
  ])
}
  ↓
取 tool_calls[0]
  ↓
call.arguments() → "{\"route\":\"充分\"}"
  ↓
serde_json::from_str → Value
  ↓
value["route"] → "充分"
```

---

## 高级用法

### 1. 并行调用（Parallel Tool Calling）

LLM 可以在一次返回中调**多个工具**，互不依赖的工具可以并行执行。

```json
{
  "tool_calls": [
    {
      "id": "call_1",
      "function": {
        "name": "search_web",
        "arguments": "{\"query\":\"Go语言最新特性2024\"}"
      }
    },
    {
      "id": "call_2",
      "function": {
        "name": "search_web",
        "arguments": "{\"query\":\"Python语言最新特性2024\"}"
      }
    }
  ]
}
```

```
LLM 一次返回两个 tool_calls
  ├── 并行执行 search_web("Go语言最新特性")
  └── 并行执行 search_web("Python语言最新特性")
  ↓
两个结果拼回 messages → 发给 LLM → LLM 继续推理
```

**条件**：模型要支持（qwen-turbo 支持，gpt-4 支持）。

### 2. 递归调用（Recursive Tool Calling）

LLM 调了工具 A，拿到结果后决定调工具 B，然后调工具 C...直到最终回答。

```
用户：分析这份财报并对比行业平均

第1轮：调 extract_data("财报.pdf")     → 返回 2024营收=100亿
第2轮：调 search_web("行业平均营收")   → 返回 行业平均=80亿
第3轮：调 analyze({"营收":100,"平均":80}) → 返回 高于平均25%
第4轮：LLM 输出最终分析报告            → content 不为 null
```

**终止条件**：`tool_calls` 为 `null` 且 `content` 不为 `null`。

### 3. 链式调用（Chained FC）

前一个 tool 的输出作为后一个 tool 的输入参数。

```
LLM 调 search("RAG 2024论文")
  → 返回论文列表
  → 代码把论文列表拼回 messages
  → LLM 看到列表后调 translate("论文标题", "中文")
  → 返回翻译结果
  → LLM 调 summarize("翻译后的内容")
  → 返回摘要
```

**关键**：每次 tool 的结果通过 `role: "tool"` 的消息拼回上下文。

### 4. Streaming + FC（流式函数调用）

边生成边通过 SSE/Stream 拿到 tool_calls。

```
SSE 事件流：
  data: {"type":"tool_call","index":0,"name":"search","arguments":"{\"query\":"}
  data: {"type":"tool_call","index":0,"arguments":"\"Go\"}"}
  data: {"type":"content","content":"根据搜索结果"}
  ...
```

**项目中**：`langchainrust` 的 `stream_chat_internal()` 支持流式 FC。

### 5. 嵌套 Schema（Nested Objects）

参数可以嵌套，不限于简单类型。

```json
{
  "name": "create_report",
  "parameters": {
    "type": "object",
    "properties": {
      "title": {"type": "string"},
      "sections": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "heading": {"type": "string"},
            "content": {"type": "string"},
            "citations": {
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "url": {"type": "string"},
                  "title": {"type": "string"}
                }
              }
            }
          }
        }
      }
    }
  }
}
```

LLM 输出：

```json
{
  "title": "Go vs Python 对比",
  "sections": [
    {
      "heading": "性能对比",
      "content": "Go在并发场景下表现更好...",
      "citations": [
        {"url": "https://...", "title": "Benchmark 2024"}
      ]
    }
  ]
}
```

### 6. 多工具选择（Multi-tool Selection）

定义多个工具让 LLM 自己选。

```json
{
  "tools": [
    {
      "name": "search_kb",
      "description": "检索企业内部知识库",
      "parameters": {...}
    },
    {
      "name": "search_web",
      "description": "搜索互联网公开信息",
      "parameters": {...}
    },
    {
      "name": "calculator",
      "description": "数学计算",
      "parameters": {...}
    }
  ]
}
```

LLM 根据任务选：问内部政策 → 调 `search_kb`；问新闻 → 调 `search_web`；算数 → 调 `calculator`。

### 7. Strict 模式

```json
{
  "name": "extract_user_info",
  "strict": true,
  "parameters": {
    "properties": {
      "name": {"type": "string"},
      "age": {"type": "number"}
    },
    "required": ["name", "age"],
    "additionalProperties": false
  }
}
```

加上 `"strict": true` 后：
- LLM 不能多输出字段（如不能加 `phone`）
- LLM 不能少输出字段（必须填 `name` + `age`）
- `additionalProperties: false` 明确禁止多余字段

**注意**：qwen-turbo **不**支持 strict 模式，GPT-4 支持。

### 8. One-of / Any-of（条件选择）

用 `oneOf` 让 LLM 根据条件选择不同的参数结构。

```json
{
  "name": "handle_request",
  "parameters": {
    "oneOf": [
      {
        "title": "查询",
        "properties": {
          "action": {"enum": ["search"]},
          "query": {"type": "string"}
        }
      },
      {
        "title": "计算",
        "properties": {
          "action": {"enum": ["calculate"]},
          "expression": {"type": "string"}
        }
      }
    ]
  }
}
```

LLM 自己选结构：查询 → `{action:"search", query:"..."}`；计算 → `{action:"calculate", expression:"..."}`。

### 9. 动态 Tool 注册（Dynamic Tool Registry）

运行时根据上下文动态决定给 LLM 暴露哪些工具。

```rust
let mut tools = Vec::new();
if user_has_permission("admin") {
    tools.push(delete_user_tool());   // 管理员可见
}
if has_docs {
    tools.push(search_docs_tool());   // 有文档库才暴露
}
llm.bind_tools(tools);
```

**项目中**：`ToolCallingEngine` 的 `extra_tools` 参数就是动态注册。

---

## 全部用法总结

### 基础分类

| 类别 | 模式 | 说明 | 项目中 |
|:----|:----|:----|:----:|
| 结构化输出 | **伪工具** | 借 FC 约束 LLM 输出格式，不执行任何代码 | `create_task_plan`、`make_decision` |
| 真工具调用 | **单次调用** | LLM 调一个工具 → 执行 → 返回结果 | `ToolCallingEngine` 聊天 |
| 真工具调用 | **并行调用** | LLM 一次返回多个 tool_calls → 同时执行 | `ToolCallingEngine` 支持 |
| 真工具调用 | **递归/链式** | 调工具→看结果→再调→直到 LLM 主动结束 | `ToolCallingEngine` 循环 |

### 参数控制

| `tool_choice` | 效果 | 场景 |
|:---|:---|:---|
| `"auto"` | LLM 自己决定调不调 | 聊天、自由对话 |
| `"required"` | 必须调一个工具 | 结构化输出、强制走 FC |
| `{"type":"function","function":{"name":"xxx"}}" | 必须调指定工具 | 指定某个工具 |
| `"none"` | 禁止调工具 | 只需要文本回答 |

### 参数 Schema 能力

| 特性 | 说明 | 例子 |
|:----|:----|:----|
| `enum` | 限制可选值 | `["充分", "不充分"]` |
| `array` | 数组类型 | `tasks` 子任务列表 |
| `nested object` | 嵌套对象 | `routes` 路由表 |
| `additionalProperties` | 动态 key-value | `routes` 的 key 是路由名 |
| `oneOf` | 条件选择结构 | 根据 `action` 字段选不同参数 |
| `strict` | 严格模式，不准多不准少 | GPT-4 支持，qwen 不支持 |
| `required` | 必填字段 | `["name", "tool", "depends_on"]` |
| `description` | 字段说明 | 告诉 LLM 这个字段填什么 |

### 执行流程控制

| 模式 | 流程 | 用途 |
|:----|:----|:----|
| 直接取值 | arguments → 解析 → 直接用 | 结构化输出（伪工具） |
| 执行并返回 | arguments → 执行工具 → 结果拼回 messages | 真工具调用 |
| 结果验证 | 执行后验证 → 不合格则重试或换工具 | 提高输出质量 |
| 回退链 | 工具 A → 失败 → 自动换工具 B | 容错 |
| Human-in-loop | 执行前等用户审批 → 点确认才执行 | 高危操作 |
| 结果缓存 | 相同参数 → 直接返回缓存 | 提速省钱 |

### 工具管理

| 方式 | 说明 |
|:----|:----|
| 静态注册 | 启动时一次性注册所有工具（`ToolRegistry::default_registry()`） |
| 动态注册 | 运行时按上下文增减工具（`ToolCallingEngine` 的 `extra_tools`） |
| MCP 发现 | 通过 MCP 协议自动发现外部工具（`McpBridge`） |
| 向量检索 | 工具太多时搜 Top-K（`ToolIndex`） |

### 项目当前使用情况

| 位置 | 用法 | 工具名 | tool_choice | 真/伪 |
|:----|:----|:----|:---:|:---:|
| `agent_executor.rs` plan() | 结构化输出 | `create_task_plan` | `required` | 伪 |
| `agent_executor.rs` decision | 结构化输出 | `make_decision` | `required` | 伪 |
| `tool_calling.rs` chat() | 真工具调用 | rag_search 等 | `auto` | 真 |
| `tool_calling.rs` chat() | 并行调用 | 多个同时 | `auto` | 真 |
| `tools.rs` registry | 静态注册 | 7 个内置工具 | - | 真 |
| `mcp_bridge.rs` | MCP 发现 | 外部工具 | - | 真 |
| `tools.rs` ToolIndex | 向量检索 | 所有工具 | - | - |

### 未实现但有价值的功能

| 功能 | 难度 | 价值 | 说明 |
|:----|:---:|:---:|:----|
| 工具结果缓存 | 低 | 高 | 相同参数重复调用时省 API 费用 |
| Human-in-loop | 中 | 高 | 高危操作让用户确认再执行 |
| 结果验证 + 自动重试 | 低 | 中 | 工具返回空/异常时自动换参数重试 |
| 回退链 | 低 | 中 | 知识库搜不到自动换网络搜 |
| 上下文感知工具选择 | 中 | 中 | 按用户角色/语境动态暴露工具 |
| Streaming FC | 中 | 低 | 流式输出同时拿到 tool_calls（收益不大） |

---

## 关键点

| 概念 | 说明 |
|------|------|
| `tools` | 定义 LLM 可以输出的结构 |
| `parameters` | JSON Schema，规定 LLM 必须怎么填 |
| `tool_choice` | 控制 LLM 调不调工具 |
| `tool_calls` | LLM 返回的结构化数据 |
| `arguments` | 参数值，JSON 字符串 |
| `content` | 用 FC 时为 null，LLM 不输出文本 |
| `enum` | 限制 LLM 只能选指定值 |
| `required` | 指定哪些字段必须填 |
