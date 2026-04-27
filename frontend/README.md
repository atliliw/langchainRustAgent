# 前端独立部署指南

## 项目结构

```
frontend/
├── index.html          # 主页面
├── nginx.conf          # Nginx 配置
├── css/
│   └── style.css       # 样式文件
└── js/
    ├── api.js          # API 请求封装
    ├── upload.js       # 上传功能
    ├── search.js       # 搜索功能
    ├── test.js         # 测试功能
    └── app.js          # 主入口
```

## 部署方式

### 方式 1: Nginx + 后端分离部署

**前端**:
- Nginx 提供静态文件服务
- 端口 80

**后端**:
- Rust Axum 服务
- 端口 8080
- 只处理 /api/* 请求

```bash
# 安装 Nginx
yum install -y nginx

# 复制配置文件
cp nginx.conf /etc/nginx/conf.d/demo.conf

# 启动 Nginx
systemctl enable nginx
systemctl start nginx
```

### 方式 2: 同一台服务器部署

使用 Nginx 同时代理前端和后端:

```
http://192.168.10.100/         → 前端静态文件
http://192.168.10.100/api/     → 后端 API
```

### 方式 3: 纯静态服务器

如果前端部署在另一台机器，修改 `js/api.js`:

```javascript
const API_BASE_URL = 'http://192.168.10.100:8080/api';
```

## API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/upload` | POST | 文件上传 |
| `/api/search` | POST | 向量搜索 |
| `/api/stats` | GET | 统计信息 |
| `/api/clear` | POST | 清空数据 |
| `/api/test/precision` | POST | 精准度测试 |
| `/api/test/cases` | GET | 测试用例 |

## 本地开发

直接用浏览器打开 index.html，修改 `api.js` 中的 API 地址即可。