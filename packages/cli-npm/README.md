# Vibe Plus CLI

本地 AI API 网关 CLI，给 Claude Code / Codex / OpenCode 等用。

控制台 <https://vibe-plus.github.io/vibe-plus/> · <a href="https://linux.do/u/cheezone" title="L 站 @cheezone"><img src="https://raw.githubusercontent.com/vibe-plus/vibe-plus/main/assets/linux-do.svg" width="20" height="20" alt="LINUX DO" align="top"></a>

## 安装

```sh
npm install -g @vibe-plus/cli
vibe
```

Bun 也行（前提是 PATH 上有 Node）：

```sh
bun install -g @vibe-plus/cli
vibe
```

npm 这层是 Node 入口，会启动平台对应的原生 `vibe` 二进制。Bun 装的用 Bun 升级，npm 装的用 npm 升级。

## 支持的 npm 平台

- macOS Apple Silicon (arm64)
- Windows x64

Intel Mac、Linux、Windows ARM64 暂未发布到 npm。

## 升级

```sh
vibe update
```

## 许可证

PolyForm Noncommercial 1.0.0 —— 见 [LICENSE](LICENSE)。

---

# Vibe Plus CLI (English)

Local AI API gateway CLI for Claude Code, Codex, OpenCode, and other OpenAI-compatible clients.

Dashboard <https://vibe-plus.github.io/vibe-plus/> · <a href="https://linux.do/u/cheezone" title="LINUX DO @cheezone"><img src="https://raw.githubusercontent.com/vibe-plus/vibe-plus/main/assets/linux-do.svg" width="20" height="20" alt="LINUX DO" align="top"></a>

## Install

```sh
npm install -g @vibe-plus/cli
vibe
```

Bun works too (when Node is on `PATH`):

```sh
bun install -g @vibe-plus/cli
vibe
```

The npm wrapper is a Node entry point that launches the platform-specific native `vibe` binary. Bun-managed installs update with Bun, npm-managed installs update with npm.

## Supported npm platforms

- macOS Apple Silicon (arm64)
- Windows x64

Intel Mac, Linux, and Windows ARM64 are not yet on npm.

## Update

```sh
vibe update
```

## License

PolyForm Noncommercial 1.0.0 — see [LICENSE](LICENSE).
