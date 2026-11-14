#!/bin/sh
# Checks one commit subject / PR title: Conventional Commits, all lowercase.
#   type(scope): summary      e.g.  feat(launcher): filter results by kind
# Usage: scripts/check-commit-title.sh "<title>"
title="$1"
types='feat|fix|refactor|perf|docs|test|ci|chore|build|style|revert'

case "$title" in
  "Merge "*|"Revert \""*|"fixup! "*|"squash! "*) exit 0 ;;
esac

if ! printf '%s' "$title" | grep -Eq "^($types)(\([a-z0-9._/-]+\))?!?: [^ ].*$"; then
  echo "✗ \"$title\""
  echo "  expected: type(scope): summary   (types: feat, fix, refactor, perf, docs, test, ci, chore, build, style, revert)"
  exit 1
fi
if printf '%s' "$title" | grep -q '[A-Z]'; then
  echo "✗ \"$title\""
  echo "  must be all lowercase"
  exit 1
fi
exit 0
