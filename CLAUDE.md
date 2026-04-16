# Flip

AI Agent 账号切换托盘应用，支持 Claude Code 和 Codex。

## 构建与开发

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 开发模式（热重载）
pnpm tauri build      # 生产构建
```

## 测试

```bash
cd src-tauri && cargo test    # Rust 后端测试
pnpm typecheck                # TypeScript 类型检查
```

## 技术栈

- Tauri 2.x (tray-only) + Rust 后端
- React 19 + TailwindCSS 4 + Framer Motion
- 存储: ~/.flip/profiles.yaml
