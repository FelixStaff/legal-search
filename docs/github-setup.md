# GitHub repository setup (recommended)

Nothing here has been configured yet; these are the recommended settings for the first public push.

## Metadata

- **Repository name**: `legal-search`
- **Description**: Local semantic search for legal PDFs. Private, offline, and page-aware.
- **Topics**: `semantic-search`, `pdf`, `legal-tech`, `nlp`, `embeddings`, `rust`, `chrome-extension`, `onnx`,
  `local-first`, `privacy`
- **Website**: leave empty until there is a Chrome Web Store listing.

After creating the repository, replace `OWNER` in `CHANGELOG.md` and `docs/chrome-web-store.md` with the account or
organization name.

## Settings

| Setting | Where | Recommendation |
|---|---|---|
| Issues | Settings → General → Features | Enabled (the issue forms in `.github/ISSUE_TEMPLATE/` are used; blank issues are disabled) |
| Private vulnerability reporting | Settings → Security → Advanced Security | **Enable** — `SECURITY.md` points to it |
| Dependabot alerts | Settings → Security → Advanced Security | Enabled |
| Dependabot security updates | Settings → Security → Advanced Security | Optional (review each PR; no automatic major upgrades) |
| Releases | Repository main page | Enabled (default). Releases are created by `.github/workflows/release.yml` only as drafts and only when the repository variable `PUBLISH_RELEASES` is `true` |
| Actions permissions | Settings → Actions → General | Allow GitHub Actions and the actions used by the workflows (listed below); default `GITHUB_TOKEN` permissions: read |
| Branch protection for `main` | Settings → Branches (or Rulesets) | Add **after the first push**: require a pull request, require the `CI` checks to pass once they have run successfully at least once, block force pushes and deletion |

## Actions used by the workflows

| Action | Version | Publisher |
|---|---|---|
| `actions/checkout` | `v7` | GitHub |
| `actions/setup-node` | `v7` | GitHub |
| `actions/cache` | `v6` | GitHub |
| `actions/upload-artifact` | `v7` | GitHub |
| `actions/download-artifact` | `v8` | GitHub |
| `dtolnay/rust-toolchain` | `stable` | David Tolnay (widely used Rust toolchain installer) |
| `Swatinem/rust-cache` | `v2` | Arpad Borsos (widely used Cargo cache) |
| `taiki-e/install-action` | `v2` | Taiki Endo (installs prebuilt `cargo-audit`) |

The workflows need no secrets for CI. They run on GitHub-hosted runners (Windows for build and tests), which are free
for public repositories. For stronger supply-chain guarantees, the action references can later be pinned to commit
SHAs.

## First push checklist

1. Review `git status` / `git ls-files --others --exclude-standard` (see `docs/release-checklist.md`).
2. Create the repository (empty, no generated README/license) and push `main`.
3. Enable private vulnerability reporting and Dependabot alerts.
4. Wait for the first `CI` run to pass, then add branch protection.
5. Tag `v1.1.0-rc2` only when the release checklist is complete.
