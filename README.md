# CC Switch Web Fork

> 本项目是基于原开源项目 **CC Switch** 的二次开发版本，目标是在保留原桌面端核心能力的基础上，增加可部署在 Linux 服务器上的 Web 运行模式。

## 原项目声明

本项目基于以下项目二次开发：

- 原项目名称：CC Switch
- 原作者：Jason Young
- 原项目仓库：https://github.com/farion1231/cc-switch
- 原项目许可证：MIT License

本仓库不是原作者官方发布版本。所有原项目版权、贡献者署名和许可证声明均应被保留。本项目新增的 Web 化改造仅代表本 fork 的二次开发方向。

## 项目简介

CC Switch Web Fork 是一个面向服务器部署的 CC Switch Web 版本。它复用了原 CC Switch 的供应商配置、账号管理、Codex OAuth、Claude/Codex/Gemini 等配置切换能力，并新增了一个 Rust HTTP 服务层，让原本依赖 Tauri 桌面 IPC 的前端可以在浏览器中运行。

当前实现方式：

- 前端仍使用原项目的 React/Vite UI。
- 桌面端运行时继续使用 Tauri API。
- Web 运行时通过 `/api/rpc/<command>` 调用 Rust 后端。
- Rust 后端新增 Web 模式，负责提供 RPC 接口和托管 `dist/` 静态文件。

## 当前功能

目前 Web 版重点支持：

- Provider 增删改查
- Provider 切换
- Universal Provider 管理与同步
- Settings 基础读取与保存
- 本地 Proxy 基础控制
- Codex OAuth 账号登录、轮询、列表、删除、默认账号设置、退出
- GitHub Copilot 账号管理相关基础能力
- 浏览器访问前端页面
- Linux 服务器后台运行

## 优点

- 可以部署到 Linux 服务器，通过浏览器访问。
- 最大限度复用原项目已有 UI 和 Rust 业务逻辑。
- 保留 Tauri 桌面端兼容性，前端同一套代码可在桌面和 Web 中运行。
- 适合集中管理服务器上的 `~/.cc-switch`、`~/.codex`、`~/.claude` 等配置。
- 对 Codex OAuth 等账号操作做了 Web RPC 适配，方便远程维护账号。

## 缺点与限制

- 当前是 Web MVP，不是原桌面版 100% 全功能迁移。
- MCP、Skills、Usage、备份导入导出、部分高级工具页面的 RPC 仍可能未完整适配。
- Web 版操作的是服务器上的配置文件，不是访问者本机的配置文件。
- 不建议直接暴露公网，因为它具备修改账号和配置的能力。
- 需要自行配置 Nginx、HTTPS、访问认证等安全措施。
- Linux 构建 Rust/Tauri 依赖较多，首次部署可能需要安装系统依赖。

## 目录说明

```text
.
├── src/                 # React/Vite 前端
├── src-tauri/           # Rust/Tauri 后端与 Web 服务
├── dist/                # 前端构建产物
├── package.json
└── README_WEB.md
```

## Linux 启动流程

以下示例以 Ubuntu/Debian 为例。

### 1. 安装系统依赖

```bash
sudo apt update
sudo apt install -y build-essential curl wget file libssl-dev \
  libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
```

### 2. 安装 Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustc --version
cargo --version
```

### 3. 安装 Node.js / pnpm

建议使用 Node.js 20 或更高版本。

```bash
corepack enable
corepack prepare pnpm@latest --activate
node -v
pnpm -v
```

### 4. 上传或克隆项目

示例路径：

```bash
sudo mkdir -p /opt/cc-switch-web
sudo chown -R "$USER":"$USER" /opt/cc-switch-web
cd /opt/cc-switch-web
```

将项目源码放入该目录。不要上传 `node_modules` 和 `src-tauri/target`，它们应在服务器上重新安装和构建。

### 5. 安装前端依赖

```bash
cd /opt/cc-switch-web
pnpm install --frozen-lockfile
```

### 6. 构建前端

```bash
pnpm build:renderer
```

构建成功后会生成：

```text
/opt/cc-switch-web/dist
```

### 7. 构建 Rust 后端

```bash
cd /opt/cc-switch-web/src-tauri
cargo build --release
```

构建成功后可执行文件位于：

```text
/opt/cc-switch-web/src-tauri/target/release/cc-switch
```

### 8. 前台测试启动

```bash
cd /opt/cc-switch-web

CC_SWITCH_WEB=1 \
CC_SWITCH_WEB_BIND=127.0.0.1:3001 \
CC_SWITCH_WEB_DIST=/opt/cc-switch-web/dist \
./src-tauri/target/release/cc-switch
```

本机测试：

```bash
curl http://127.0.0.1:3001/api/health
```

如果返回类似内容，说明服务启动成功：

```json
{ "ok": true, "name": "cc-switch-web" }
```

### 9. 临时开放访问

如果只是在内网测试，可以绑定到所有网卡：

```bash
CC_SWITCH_WEB=1 \
CC_SWITCH_WEB_BIND=0.0.0.0:3001 \
CC_SWITCH_WEB_DIST=/opt/cc-switch-web/dist \
./src-tauri/target/release/cc-switch
```

然后访问：

```text
http://服务器IP:3001
```

不建议生产环境直接这样暴露。

## systemd 后台运行

创建服务文件：

```bash
sudo nano /etc/systemd/system/cc-switch-web.service
```

写入：

```ini
[Unit]
Description=CC Switch Web Fork
After=network.target

[Service]
Type=simple
User=你的Linux用户名
WorkingDirectory=/opt/cc-switch-web
Environment=CC_SWITCH_WEB=1
Environment=CC_SWITCH_WEB_BIND=127.0.0.1:3001
Environment=CC_SWITCH_WEB_DIST=/opt/cc-switch-web/dist
ExecStart=/opt/cc-switch-web/src-tauri/target/release/cc-switch
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

启动服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable cc-switch-web
sudo systemctl start cc-switch-web
sudo systemctl status cc-switch-web
```

查看日志：

```bash
journalctl -u cc-switch-web -f
```

## Nginx 反向代理示例

建议只让 Rust 服务监听 `127.0.0.1:3001`，公网访问走 Nginx，并配置 HTTPS 和认证。

```nginx
server {
    listen 80;
    server_name your-domain.com;

    location / {
        proxy_pass http://127.0.0.1:3001;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

启用后：

```bash
sudo nginx -t
sudo systemctl reload nginx
```

## 环境变量

| 变量                 | 说明                                    | 默认值              |
| -------------------- | --------------------------------------- | ------------------- |
| `CC_SWITCH_WEB`      | 是否启用 Web 模式，设置为 `1` 或 `true` | 未启用              |
| `CC_SWITCH_WEB_BIND` | Web 服务监听地址                        | `127.0.0.1:3001`    |
| `CC_SWITCH_WEB_DIST` | 前端静态文件目录                        | 当前目录下的 `dist` |

也可以使用启动参数：

```bash
./src-tauri/target/release/cc-switch --web
```

## 数据与配置位置

Web 版操作的是服务器当前运行用户的配置，例如：

```text
~/.cc-switch/
~/.codex/
~/.claude/
~/.gemini/
```

如果使用 systemd，请注意 `User=` 对应哪个 Linux 用户。不同用户会有不同的 home 目录和配置文件。

## 安全建议

- 不要裸露到公网。
- 使用 Nginx + HTTPS。
- 增加 Basic Auth、OAuth Proxy、VPN 或内网访问控制。
- 单独创建低权限 Linux 用户运行服务。
- 定期备份 `~/.cc-switch`、`~/.codex`、`~/.claude`。
- 不要把包含 token、auth.json、数据库文件的目录提交到 Git 仓库。

## 开发模式

前端开发：

```bash
pnpm dev:renderer
```

后端 Web 模式：

```bash
cd src-tauri
CC_SWITCH_WEB=1 CC_SWITCH_WEB_DIST=../dist cargo run -- --web
```

如果前端开发服务器和后端端口不同，可以通过 `VITE_CC_SWITCH_API_BASE` 指向后端：

```bash
VITE_CC_SWITCH_API_BASE=http://127.0.0.1:3001 pnpm dev:renderer
```

## 开源说明

本项目应继续遵循原项目的 MIT License。开源发布时请保留：

- 原项目 LICENSE
- 原作者与原仓库链接
- 本 README 中的二次开发声明
- 原项目贡献者相关署名

如需发布二进制版本，请在发布页明确说明这是 fork 版本，不是原作者官方构建。
