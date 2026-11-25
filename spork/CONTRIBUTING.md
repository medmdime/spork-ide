# Contributing to Spork

Thanks for helping build Spork. Read this once before your first pull request.

## Branches

| Branch | Purpose |
|---|---|
| `main` | **Production.** Only released, stable code. Every release is a tag on `main`. |
| `develop` | Integration branch. All finished work lands here first. |
| `feat/<topic>` | One branch per feature, created from `develop`, merged back into `develop` by pull request. |
| `fix/<topic>` | Same, for bug fixes. |

Flow: `feat/<topic>` → pull request → `develop` → release pull request → `main` (+ tag).

## Rules

1. **Every push runs all test suites.** CI runs formatting, lint (`clippy -D warnings`) and `cargo test --workspace`
   on macOS, Linux and Windows for every push to every branch. A branch is mergeable only when all of them pass.
2. **Every change has its own branch.** Don't commit straight to `develop` or `main`; open a `feat/…` or `fix/…`
   branch and a pull request.
3. **Commit messages follow [Conventional Commits](https://www.conventionalcommits.org)**: `feat: …`, `fix: …`,
   `docs: …`, `refactor: …`, `test: …`, `ci: …`, `chore: …`.
4. **Commit messages contain the message only.** No `Co-Authored-By` or tool attribution lines.
   The one required trailer is the DCO sign-off: commit with `git commit -s`.
5. **Clean-room.** No code, pseudo-code or text derived from decompiling or disassembling proprietary software.
   Specs describe observable behaviour; code is written from specs. See [docs/20-open-source.md](docs/20-open-source.md).

## When these rules are enforced

Spork is in early development, and for now the work happens directly on `main`. **Once the app is stable**, the
`develop` branch is created and GitHub enforces the rules above: protected `main` and `develop`, required CI checks,
pull requests only, no force pushes. Until then, rules 3–5 already apply to every commit.

## Getting started

Run exactly what CI runs (format, lint, all tests) before every push:

```bash
./scripts/check.sh
```

```bash
cargo run -p spork -- /path/to/a/repo
```

Pick a module spec in [`docs/specs/`](docs/specs), open an issue or draft PR to say what you're working on, and go.
