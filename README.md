# git-squish

A Git utility that squashes commits on a branch into a single commit against an upstream branch.

## Installation

Install via Cargo:

```bash
cargo install git-squish
```

## Usage

Basic usage:

```bash
# Squash current branch onto origin/main
git squish main

# Squash specific branch onto upstream
git squish topic main
```

### Arguments

- `[branch-refname]` - Optional. The branch to squash (e.g., "refs/heads/feature"). If omitted, uses the current branch.
- `<upstream-spec>` - Required. The upstream to rebase onto (e.g., "main" or "origin/main").

## GPG Signing Support

git-squish will respect your existing Git GPG signing configuration. If you have GPG signing enabled in your Git config, the squashed commit will be signed automatically.