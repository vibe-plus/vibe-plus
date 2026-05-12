# Release and Update Runbook

Vibe Plus should have two release channels:

- **npm CLI channel**: the default channel now. Users install `@vibe-plus/cli`,
  which resolves a native platform package such as `@vibe-plus/darwin-arm64`.
- **standalone app/installer channel**: future channel for users without Node,
  inspired by Codex standalone installers and CC Switch desktop assets.

The current repository is wired for the npm CLI channel.

## What We Borrowed

CC Switch keeps release simple: push a `v*` tag, build platform artifacts in CI,
and attach/publish the outputs. That is the right shape for a small product.

Codex adds two useful hardening ideas:

- validate that the tag version matches the source version before building
- keep installation/update as a product workflow, not only as a CI workflow

Vibe Plus follows both: `.github/workflows/release.yml` validates the tag
against `Cargo.toml` and `packages/cli-npm/package.json`, then publishes platform
packages before the wrapper. The CLI exposes `vibe update`.

## Version Rules

Use SemVer tags:

```text
v0.1.0
v0.1.1
v0.2.0-alpha.1
```

The tag without `v` must match:

- `[workspace.package].version` in `Cargo.toml`
- `version` in `packages/cli-npm/package.json`

The release workflow stamps the platform package versions from the tag at
publish time, but keeping local package files aligned makes dry runs and reviews
less surprising.

## Preflight

Run these from the repo root before tagging:

```bash
vp check
vp run -r test
vp run -r build
cargo test --workspace
cd packages/cli-npm && npm run verify
```

For a first release or package layout change, also do a local release-shape check
with real binaries:

```bash
cargo build --release -p vibe
mkdir -p packages/cli-npm/platform/$(node -p '`${process.platform}-${process.arch}`')/bin
cp target/release/vibe packages/cli-npm/platform/$(node -p '`${process.platform}-${process.arch}`')/bin/vibe
cd packages/cli-npm && npm run verify -- --require-binaries
```

On Windows, copy `target\release\vibe.exe` into the matching
`platform\win32-*\bin\vibe.exe` directory.

## Release Steps

1. Update versions in source:

   ```bash
   VERSION=0.1.0
   ```

   Set `Cargo.toml` workspace version and `packages/cli-npm/package.json`
   version to `$VERSION`. Keep platform package JSON files at the same version
   unless the release workflow is the only publisher touching them.

2. Update release notes or the README if user-facing behavior changed.

3. Run preflight.

4. Commit the version bump:

   ```bash
   git add Cargo.toml Cargo.lock packages/cli-npm/package.json packages/cli-npm/platform/*/package.json README.md
   git commit -m "chore(release): prepare v$VERSION"
   ```

5. Create and push the tag:

   ```bash
   git tag -a "v$VERSION" -m "Release v$VERSION"
   git push origin main
   git push origin "v$VERSION"
   ```

6. Watch the GitHub Actions release workflow.

7. Verify npm after CI completes:

   ```bash
   npm view @vibe-plus/cli@$VERSION version
   npm view @vibe-plus/darwin-arm64@$VERSION version
   npm view @vibe-plus/darwin-x64@$VERSION version
   npm view @vibe-plus/linux-arm64@$VERSION version
   npm view @vibe-plus/linux-x64@$VERSION version
   npm view @vibe-plus/win32-arm64@$VERSION version
   npm view @vibe-plus/win32-x64@$VERSION version
   ```

8. Smoke test install on at least one machine:

   ```bash
   npm install -g @vibe-plus/cli@$VERSION
   vibe --version
   vibe doctor
   ```

## User Update Flow

For npm installs:

```bash
vibe update
```

The npm wrapper marks the launched binary with `VIBE_MANAGED_BY_NPM=1` or
`VIBE_MANAGED_BY_BUN=1`, so `vibe update` can use the same package manager:

- npm-managed install: `npm install -g @vibe-plus/cli@latest`
- Bun-managed install: `bun install -g @vibe-plus/cli@latest`

Manual equivalents:

```bash
npm install -g @vibe-plus/cli@latest
bun install -g @vibe-plus/cli@latest
```

For pinned versions:

```bash
npm install -g @vibe-plus/cli@0.1.0
```

## Rollback and Failed Releases

npm package versions are immutable. If a bad version is published, do not try to
republish it.

Use one of these paths:

- publish a patch version, for example `v0.1.1`
- deprecate the bad package version:

  ```bash
  npm deprecate @vibe-plus/cli@0.1.0 "Use 0.1.1 or newer"
  ```

- if the GitHub release was created but npm failed, fix the workflow/package
  issue and rerun the failed job only if no package version needs replacement

If platform packages were published but the wrapper failed, rerunning publish is
safe because the workflow skips packages that already exist.

## Future Standalone Channel

Add this when Node-free installation matters:

- build compressed GitHub release archives for each platform
- include SHA-256 digests in release metadata
- add `scripts/install/install.sh` and `scripts/install/install.ps1`
- teach `vibe update` to detect standalone installs and download the latest
  GitHub release asset with digest verification

Codex is the model here: install into a versioned directory, update a `current`
symlink or shim atomically, and keep a small lock around update operations.
