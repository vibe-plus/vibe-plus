# Vibe Plus

Vibe Plus is a local AI API gateway for developer tools such as Claude Code,
Codex, OpenCode, and OpenAI-compatible clients.

The product is intentionally CLI-first:

- `crates/vibe-core` owns routing, provider adapters, failover, logs, config,
  and the local HTTP API.
- `crates/vibe` is the durable product interface. It can configure providers,
  start the gateway, patch client tools, inspect logs, and run subprocesses with
  proxy environment variables.
- `apps/website` is an optional local console. It must stay disposable: if it is
  deleted, the gateway and CLI should still work.
- Future desktop or hosted apps should call the same local HTTP API and/or shell
  out to the CLI. They should not own provider credentials, routing policy, log
  storage, or client takeover logic.

Transparency rule:

- The App may observe local shell UX signals such as paste, drag/drop, clipboard
  candidates, and live gateway status.
- The App must not become the source of truth. Importing credentials, saving
  Codex config, editing routes, changing gateway settings, and startup behavior
  are all owned by the CLI/core API.
- Anything the App can mutate should also be available through `vibe` commands
  or the local gateway API.

## CLI-First Workflow

Install the released CLI with npm:

```bash
npm install -g @vibe-plus/cli
vibe --version
```

You can also install it with Bun when Node is available on `PATH`:

```bash
bun install -g @vibe-plus/cli
vibe --version
```

`vibe update` uses the package manager that launched the npm wrapper, so
npm-managed installs update with npm and Bun-managed installs update with Bun.
The npm wrapper itself is a Node entry point; users with Bun but no Node should
use a standalone binary/installer once that distribution channel is available.

The npm release currently publishes native binaries for:

- macOS arm64/x64
- Linux arm64/x64
- Windows arm64/x64

Other targets, including 32-bit Linux ARM, need their own tested Rust target and
release package before they are advertised as supported.

Build the CLI:

```bash
cargo build -p vibe
```

Start the gateway:

```bash
cargo run -p vibe -- start --foreground
```

Start on login without the App:

```bash
cargo run -p vibe -- autostart enable
cargo run -p vibe -- autostart status
cargo run -p vibe -- autostart disable
```

On macOS this installs a user LaunchAgent that runs `vibe start --foreground`.
The App is not required for startup.

Inspect health without opening the web UI:

```bash
cargo run -p vibe -- doctor
cargo run -p vibe -- status
cargo run -p vibe -- provider list
cargo run -p vibe -- route list
cargo run -p vibe -- logs --limit 20
```

Point a tool at the local gateway:

```bash
cargo run -p vibe -- takeover claude
cargo run -p vibe -- takeover codex
cargo run -p vibe -- takeover opencode
```

Run any OpenAI/Anthropic-compatible command through Vibe Plus:

```bash
cargo run -p vibe -- run -- <command> [args...]
```

## Optional Web Console

The website is useful during development, but it is not the product kernel.
It runs on port `15876` in development to avoid common Vite ports.

```bash
vp run dev
```

The console should use only these public seams:

- local gateway endpoints under `http://127.0.0.1:<port>`
- generated protocol types in `packages/protocol`
- CLI commands for workflows that already exist in `crates/vibe`

## App Shell

During development, any desktop App shell should load the existing frontend:

```text
http://127.0.0.1:15876
```

Run the disposable desktop shell:

```bash
cargo run -p vibe-app
```

The shell defaults to the development console above. Override it only when
pointing at a preview or packaged frontend:

```bash
cargo run -p vibe-app -- --url http://127.0.0.1:15876
```

Desktop-shell controls:

```bash
cargo run -p vibe-app -- --floating
cargo run -p vibe-app -- --width 900 --height 640
cargo run -p vibe-app -- --always-on-top
cargo run -p vibe-app -- --frameless --transparent
cargo run -p vibe-app -- --floating --no-hide-on-blur
cargo run -p vibe-app -- --no-tray
```

The shell currently owns only desktop affordances:

- a native window loading the frontend
- a tray icon that toggles the window on click
- tray menu actions: Show Vibe Plus, Hide, Quit Vibe Plus
- close-to-hide behavior so the tray can bring the window back; on macOS, hiding
  also hides the application so focus returns to the previous app
- `Cmd+Q` through the tray menu accelerator exits the App
- external open/deeplink events surfaced to the shell event loop
- optional floating-window flags (`--floating`, `--hide-on-blur`,
  `--no-hide-on-blur`, `--always-on-top`, `--frameless`, `--transparent`)

`--floating` uses the smaller floating-window minimum size and hides on blur by
default. It does not imply always-on-top, frameless, or transparency; those must
be requested explicitly. If the tray is disabled with `--no-tray`, closing the
window exits the App instead of hiding it.

Do not embed a second UI implementation inside the App shell. The shell can own
window chrome, tray/menu actions, clipboard observation, drag/drop, and startup
entry points, but all product state mutations still go through `vibe` or the
local gateway API. In production, the same shell can load the built
`apps/website/dist` assets.

## Configuration

Vibe stores local state under `~/.vibe` by default:

- `config.toml` for server, failover, and logging options
- `vibe.db` for providers, credentials, health, usage, and request logs
- `backups/` for client config backups created by `vibe takeover`

Set `VIBE_HOME` to isolate state for tests or development:

```bash
VIBE_HOME=/tmp/vibe-dev cargo run -p vibe -- doctor
```

## Validation

Run Rust checks:

```bash
cargo test --workspace
```

Run frontend and package checks when touching TypeScript or Vue code:

```bash
vp check
vp run -r test
vp run -r build
```

Run black-box gateway tests against a running gateway:

```bash
python3 tests/e2e_blackbox_gateway.py --gateway http://127.0.0.1:15917
```

## Release

Pushing a `v*` tag builds the Rust CLI for macOS, Linux, and Windows, publishes
the platform binary packages under `@vibe-plus/*`, then publishes the
`@vibe-plus/cli` npm wrapper.

Before the first npm release, make sure the npm account behind `NPM_TOKEN` can
publish public packages under the `@vibe-plus` scope.

See [docs/release.md](docs/release.md) for the full release and update runbook.
