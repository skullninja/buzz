#!/usr/bin/env bash
# Regression test for the worktree detection in scripts/instance-env.sh.
#
# The app identifier — and therefore the application-support directory holding
# the user's communities, agents and keys — is suffixed with the branch name
# ONLY in a worktree. Detection compares --git-dir against --git-common-dir.
#
# git prints those paths in whichever form is shortest relative to the CURRENT
# directory. From a subdirectory of a plain checkout it returns an absolute
# --git-dir and a relative --git-common-dir for the same directory, so a raw
# string comparison reports "worktree" for every ordinary checkout. `just dev`
# sources the script from desktop/, so this fired every time a developer worked
# on a branch: the app launched as a fresh install with no community or agents.
#
# These tests pin both directions from a subdirectory, which is the case that
# regressed.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="$repo_root/scripts/instance-env.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

failures=0
fail() {
  printf 'FAIL: %s\n' "$1" >&2
  failures=$((failures + 1))
}
pass() {
  printf 'ok: %s\n' "$1"
}

# Report BUZZ_INSTANCE_SLUG as instance-env.sh would set it, sourced from $1.
slug_from() {
  (
    cd "$1"
    set +u
    # shellcheck disable=SC1090
    . "$script" >/dev/null 2>&1
    printf '%s' "${BUZZ_INSTANCE_SLUG:-}"
  )
}

# A checkout with a subdirectory, on a non-main branch so a leaked slug is visible.
main_checkout="$tmp/checkout"
mkdir -p "$main_checkout/sub"
git -C "$main_checkout" init -q
git -C "$main_checkout" config user.email test@example.com
git -C "$main_checkout" config user.name Test
git -C "$main_checkout" commit -q --allow-empty -m init
git -C "$main_checkout" checkout -q -b feature-branch

# 1. Plain checkout, from the repo root.
if [[ -z "$(slug_from "$main_checkout")" ]]; then
  pass "plain checkout at root sets no slug"
else
  fail "plain checkout at root set a slug: $(slug_from "$main_checkout")"
fi

# 2. Plain checkout, from a SUBDIRECTORY — the regression.
if [[ -z "$(slug_from "$main_checkout/sub")" ]]; then
  pass "plain checkout from a subdirectory sets no slug"
else
  fail "plain checkout from a subdirectory set a slug: $(slug_from "$main_checkout/sub")"
fi

# 3. A real worktree must still be detected — from its root and a subdirectory.
worktree="$tmp/wt"
git -C "$main_checkout" worktree add -q -b wt-branch "$worktree"
mkdir -p "$worktree/sub"

if [[ "$(slug_from "$worktree")" == "wt-branch" ]]; then
  pass "worktree at root sets the branch slug"
else
  fail "worktree at root slug was '$(slug_from "$worktree")', expected wt-branch"
fi

if [[ "$(slug_from "$worktree/sub")" == "wt-branch" ]]; then
  pass "worktree from a subdirectory sets the branch slug"
else
  fail "worktree from a subdirectory slug was '$(slug_from "$worktree/sub")', expected wt-branch"
fi

if [[ "$failures" -gt 0 ]]; then
  printf '\n%d test(s) failed\n' "$failures" >&2
  exit 1
fi
printf '\nall tests passed\n'
