# Release, Install, and Update Runbook

Vibe Plus is a user-facing desktop product first. The npm packages are useful
for developers, CI, and fast mirrored downloads, but they are not the primary
installation story.

The target user experience is:

1. A Windows or macOS user downloads one installer or runs one install command.
2. Vibe Plus installs the App and the bundled gateway binary.
3. First launch offers a single "Set up Vibe Plus" flow.
4. That flow can enable autostart, migrate existing local auth/config, and take
   over supported tools with backups.
5. Future updates happen inside the App.

## What CC Switch Does

CC Switch is the closest model for the App channel.

- It ships real desktop artifacts from GitHub Releases:
  - macOS `.dmg` and `.zip`
  - Windows `.msi` and portable `.zip`
  - Linux `.AppImage`, `.deb`, and `.rpm`
- It uses Tauri 2 bundling with `createUpdaterArtifacts = true`.
- It signs updater artifacts and uploads `.sig` files.
- It generates `latest.json` in the GitHub release for the Tauri updater.
- The App checks for updates at launch and exposes update controls in Settings.
- On Windows it customizes WiX for a per-user installer and elevated update
  task support.
- On macOS it signs and notarizes both the `.app` and `.dmg`.

For Vibe Plus, this means the App release should eventually be the canonical
release, not the npm wrapper.

## What Codex Does

Codex is the best model for a standalone binary installer.

- `install.sh` and `install.ps1` download GitHub release assets directly.
- Assets are selected by OS and CPU.
- Downloads are verified with SHA-256 digests from GitHub release metadata.
- Releases are installed into a versioned directory.
- A `current` symlink or junction is updated atomically.
- Installer locks prevent two updates from racing.
- Existing npm/bun/homebrew installs are detected so PATH conflicts are visible.

For Vibe Plus, this is useful for a future CLI-only or emergency repair channel.
It is not as good as a desktop installer for non-technical users.

## Recommended Channels

Use three channels, in this order of importance:

1. **Desktop App channel**
   - Primary user channel.
   - Ships installer artifacts and in-App updates.
   - Should not require Node, Bun, npm, cargo, or a terminal.

2. **Standalone repair channel**
   - One-line script for advanced users and support docs.
   - Installs or repairs the bundled `vibe` gateway binary.
   - Can be used when the App updater is broken.

3. **npm CLI channel**
   - Developer and CI channel.
   - Useful with npmmirror.
   - Not user-facing in product copy.

## Desktop Architecture

The App should bundle or install the `vibe` gateway binary next to the desktop
shell. The shell is responsible for desktop affordances only:

- first-run setup
- tray/menu and window lifecycle
- update UI
- invoking the bundled gateway binary
- showing progress and errors

The gateway remains the product kernel:

- provider import and migration
- route/config edits
- autostart setup
- client takeover and restore
- logs and health checks

That keeps CLI, App, and future installers aligned.

## First-Run Setup Flow

On first launch, show one guided setup with explicit consent:

1. **Detect**
   - Find existing Codex, Claude Code, OpenCode, and compatible config/auth
     paths.
   - Show what was found before modifying anything.

2. **Migrate**
   - Import supported local credentials/config into Vibe Plus.
   - Keep backups under `~/.vibe/backups/`.
   - Make each migration idempotent so rerunning setup is safe.

3. **Autostart**
   - Enable `vibe start --foreground` on login.
   - macOS: user LaunchAgent.
   - Windows: per-user startup task or Run key; Task Scheduler is better for
     reliable hidden startup.

4. **Takeover**
   - Offer per-client takeover toggles.
   - Default can be "recommended selected", but the user should confirm because
     takeover rewrites local client config.
   - Always create backups and expose restore.

5. **Verify**
   - Start gateway.
   - Run health/status checks.
   - Show "Ready" only when the gateway is reachable and selected clients are
     configured.

This can be implemented by adding a single backend command such as
`vibe setup --json` or local API endpoint that orchestrates existing
`autostart`, `takeover`, and import functions.

## In-App Update Flow

For the App channel, use Tauri updater:

- `tauri.conf.json` enables `bundle.createUpdaterArtifacts`.
- The release workflow uploads signed updater artifacts and a `latest.json`.
- The App checks `latest.json` on startup and from Settings.
- When an update is accepted, the App downloads, verifies, installs, and
  relaunches.

Important product behavior:

- If the gateway is running, stop it before replacing bundled binaries.
- After relaunch, start the gateway again if autostart/setup had enabled it.
- Run a lightweight migration step after version changes.
- Keep update failures recoverable by opening the GitHub release download page.

## Release Artifacts

Initial user-facing targets:

- macOS Apple Silicon `.dmg`
- macOS Intel `.dmg`
- Windows x64 `.msi`
- Windows arm64 `.msi`

Later:

- Linux `.AppImage`/`.deb`/`.rpm`
- portable zip packages
- standalone CLI archives

macOS must be signed and notarized before broad distribution. Windows can start
unsigned for internal testing, but SmartScreen friction will be high until code
signing is added.

## Release Steps

1. Update versions:
   - `Cargo.toml` workspace version
   - App bundle version/config
   - `packages/cli-npm/package.json` for the developer channel

2. Run validation:

   ```bash
   vp check
   vp run -r test
   vp run -r build
   cargo test --workspace
   ```

3. Build local smoke artifacts for the current platform.

4. Commit the release prep.

5. Push a SemVer tag:

   ```bash
   VERSION=0.1.0
   git tag -a "v$VERSION" -m "Release v$VERSION"
   git push origin main
   git push origin "v$VERSION"
   ```

6. CI builds and uploads:
   - user installers
   - updater artifacts and signatures
   - `latest.json`
   - npm packages as the secondary developer channel

7. Smoke test:
   - install on a clean Windows account
   - install on a clean macOS account
   - run first-run setup
   - confirm autostart after logout/login
   - confirm client takeover and restore
   - confirm App update from the previous version

## Implementation Gap

Current Vibe Plus state:

- Has a Rust gateway CLI.
- Has a disposable desktop shell.
- Has npm platform packages and `vibe update`.
- Does not yet have a true packaged App installer.
- Does not yet have Tauri updater metadata/signing.
- Does not yet have a single first-run setup orchestration command.

Recommended next implementation milestones:

1. Convert the desktop shell to a real Tauri 2 app bundle that embeds the
   website build and bundled `vibe` binary.
2. Add `vibe setup` as an idempotent JSON-producing orchestration command.
3. Add App UI for first-run setup and in-App update.
4. Add GitHub release artifacts: `.dmg`, `.msi`, updater archives, signatures,
   `latest.json`.
5. Keep npm release as a secondary job.

## Rollback

Installer versions are immutable once published. If a bad release ships:

- publish a patch release
- mark the GitHub release as prerelease or add a warning
- deprecate the npm wrapper version if needed
- keep restore paths for client takeover backups
