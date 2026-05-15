# Vibe Plus CLI

Local AI API gateway CLI for developer tools such as Claude Code, Codex,
OpenCode, and OpenAI-compatible clients.

## Install

```sh
npm install -g @vibe-plus/cli
vibe --version
```

Bun can be used as the package manager when Node is available on `PATH`:

```sh
bun install -g @vibe-plus/cli
vibe --version
```

The npm wrapper is a Node entry point that launches a platform-specific native
`vibe` binary. Bun-managed installs update with Bun, npm-managed installs update
with npm.

## Supported npm platforms

- macOS Apple Silicon (arm64)
- Windows x64

Intel Mac, Linux, and Windows ARM64 are not published on npm yet. Build from
source or use a future standalone installer for those platforms.

## Update

```sh
vibe update
```
