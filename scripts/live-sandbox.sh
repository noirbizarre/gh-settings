#!/usr/bin/env bash
# Create or repair a sandbox repository for the live suite.
#
# The live tests mutate a real repository, and `Live::preflight()` refuses to
# start against one that already holds managed configuration. That refusal is
# correct but leaves you stranded, so this is the way back: it provisions a
# sandbox that does not exist yet, and resets one that a crashed run left dirty.
#
# It talks to `gh` directly rather than to gh-settings. A repair tool that
# depends on the thing being repaired is useless on the day it matters.
#
#   scripts/live-sandbox.sh [owner/repo] [--yes]
#
# The repository defaults to $GH_SETTINGS_TEST_REPO. See CONTRIBUTING.md: CI's
# sandbox belongs to CI, so bring your own.
set -euo pipefail

repo="${GH_SETTINGS_TEST_REPO:-}"
assume_yes=0

for arg in "$@"; do
  case "$arg" in
    --yes | -y) assume_yes=1 ;;
    -h | --help)
      sed -n '2,15p' "${BASH_SOURCE[0]}" | sed 's|^# \{0,1\}||'
      exit 0
      ;;
    -*)
      echo "unknown option: $arg" >&2
      exit 2
      ;;
    *) repo="$arg" ;;
  esac
done

die() {
  echo "error: $*" >&2
  exit 1
}

if [[ -z $repo ]]; then
  die "no repository given: pass one, or set GH_SETTINGS_TEST_REPO"
fi

if [[ $repo != */* || $repo == */*/* ]]; then
  die "$repo is not an \`owner/repo\` pair"
fi

# `gh api` on a paginated endpoint concatenates pages, so the arrays below are
# read one page at a time with an explicit `--paginate --slurp` only where it
# could matter. A sandbox never has enough of anything to paginate.
api() { gh api "$@"; }

# --- Create -----------------------------------------------------------------

if gh repo view "$repo" --json name >/dev/null 2>&1; then
  existed=1
else
  existed=0
fi

if ((existed)); then
  echo "About to RESET $repo:"
  echo "  every ruleset, autolink, environment and Actions variable is deleted"
  echo "  every non-default label is deleted, topics are cleared"
  echo "  description and homepage are cleared, Pages disabled if it can be"
else
  echo "About to CREATE $repo as a public repository."
fi

if ((!assume_yes)); then
  read -r -p "Continue? [y/N] " reply
  [[ $reply == [yY]* ]] || die "aborted"
fi

if ((!existed)); then
  gh repo create "$repo" \
    --public \
    --add-readme \
    --description "Sandbox for the gh-settings live suite"
  echo "created $repo"
fi

# Rulesets answer `403 Upgrade to GitHub Pro` on a private repository on the
# free plan, so a private sandbox silently loses the coverage the live suite
# exists for. The pre-flight says the same thing; better to hear it here.
visibility="$(gh repo view "$repo" --json visibility --jq .visibility)"
if [[ $visibility != PUBLIC ]]; then
  die "$repo is $visibility. Rulesets need GitHub Pro on a private repository, so the sandbox must be public."
fi

# --- Seed -------------------------------------------------------------------

# `live_pages_enable_and_update` builds Pages from a `gh-pages` branch, and
# GitHub rejects a source branch that does not exist.
default_branch="$(gh repo view "$repo" --json defaultBranchRef --jq .defaultBranchRef.name)"
if [[ -z $default_branch || $default_branch == null ]]; then
  die "$repo has no default branch. Push a commit to it, then run this again."
fi

if ! api "repos/$repo/git/ref/heads/gh-pages" >/dev/null 2>&1; then
  head="$(api "repos/$repo/git/ref/heads/$default_branch" --jq .object.sha)"
  api "repos/$repo/git/refs" \
    --method POST \
    --field ref=refs/heads/gh-pages \
    --field sha="$head" >/dev/null
  echo "created branch gh-pages"
fi

# `live_pages_enable_and_update` publishes from `gh-pages` `/docs`. Without that
# directory the build never completes, and GitHub refuses to deactivate a site
# while a build is outstanding — so an empty branch makes Pages undeletable for
# ever, not just for a minute.
if ! api "repos/$repo/contents/docs/index.html?ref=gh-pages" >/dev/null 2>&1; then
  api "repos/$repo/contents/docs/index.html" \
    --method PUT \
    --field message="Seed the Pages source for the live suite" \
    --field branch=gh-pages \
    --field content="$(printf '<!doctype html><title>gh-settings live sandbox</title>' | base64 -w0)" \
    >/dev/null
  echo "seeded gh-pages:/docs/index.html"
fi

# --- Reset ------------------------------------------------------------------

# Everything below must be idempotent: this runs against a repository in an
# unknown state, which is the whole point.
#
# And everything below is best-effort. `set -e` is right for the sections
# above — a sandbox that could not be created or seeded is not a sandbox — but
# wrong here: a reset that stops at the first refusal reports failure *after*
# most of the destruction, leaving you unable to tell what state you are in.
# So each step warns and carries on, and the exit code is decided at the end.

failures=()

warn() {
  failures+=("$1")
  echo "warning: $1" >&2
}

# Run a destructive call, recording the failure instead of aborting.
try() {
  local what="$1"
  shift
  local output
  if output="$("$@" 2>&1)"; then
    return 0
  fi
  warn "$what: ${output//$'\n'/ }"
  return 1
}

for id in $(api "repos/$repo/rulesets" --jq '.[].id' 2>/dev/null || true); do
  if try "deleting ruleset $id" gh api "repos/$repo/rulesets/$id" --method DELETE --silent; then
    echo "deleted ruleset $id"
  fi
done

for id in $(api "repos/$repo/autolinks" --jq '.[].id' 2>/dev/null || true); do
  if try "deleting autolink $id" gh api "repos/$repo/autolinks/$id" --method DELETE --silent; then
    echo "deleted autolink $id"
  fi
done

for name in $(api "repos/$repo/environments" --jq '.environments[].name' 2>/dev/null || true); do
  if try "deleting environment $name" gh api "repos/$repo/environments/$name" --method DELETE --silent; then
    echo "deleted environment $name"
  fi
done

for name in $(api "repos/$repo/actions/variables" --jq '.variables[].name' 2>/dev/null || true); do
  if try "deleting variable $name" gh api "repos/$repo/actions/variables/$name" --method DELETE --silent; then
    echo "deleted variable $name"
  fi
done

# Labels are the one resource GitHub creates for you, so "clean" means "only
# the defaults". This list matches DEFAULTS in tests/common/live.rs.
defaults=(
  "bug" "documentation" "duplicate" "enhancement" "good first issue"
  "help wanted" "invalid" "question" "wontfix"
)
while IFS= read -r name; do
  [[ -n $name ]] || continue
  keep=0
  for default in "${defaults[@]}"; do
    if [[ $name == "$default" ]]; then
      keep=1
      break
    fi
  done
  if ((keep)); then
    continue
  fi
  # A label name may contain spaces, and `gh api` does not encode the path.
  if try "deleting label $name" gh api "repos/$repo/labels/${name// /%20}" --method DELETE --silent; then
    echo "deleted label $name"
  fi
done < <(api "repos/$repo/labels" --jq '.[].name' 2>/dev/null || true)

topics="$(api "repos/$repo/topics" --jq '.names | length' 2>/dev/null || echo 0)"
if [[ $topics != 0 ]]; then
  if try "clearing topics" gh api "repos/$repo/topics" --method PUT --input - --silent \
    <<<'{"names":[]}'; then
    echo "cleared topics"
  fi
fi

# The live suite clears Pages and the repository fields itself, but a run killed
# outright leaves them, and the pre-flight cannot see either.
#
# Pages is the awkward one. On a public repository GitHub ties the site to the
# `gh-pages` branch and answers `422 Deactivating GitHub pages for this
# repository is not allowed` for as long as that branch exists — verified by
# deleting the branch, at which point the site vanished on its own. Since the
# sandbox needs that branch for `live_pages_enable_and_update`, a site here is
# not residue to be cleaned; it is the sandbox working as intended.
if api "repos/$repo/git/ref/heads/gh-pages" >/dev/null 2>&1; then
  if api "repos/$repo/pages" >/dev/null 2>&1; then
    echo "left Pages enabled: it cannot be disabled while gh-pages exists"
  fi
elif api "repos/$repo/pages" >/dev/null 2>&1; then
  if try "disabling Pages" gh api "repos/$repo/pages" --method DELETE --silent; then
    echo "disabled Pages"
  fi
fi

if try "clearing description and homepage" \
  gh api "repos/$repo" --method PATCH --field description= --field homepage= --silent; then
  echo "cleared description and homepage"
fi

# --- Done -------------------------------------------------------------------

if ((${#failures[@]})); then
  echo >&2
  echo "${#failures[@]} step(s) did not succeed:" >&2
  printf '  %s\n' "${failures[@]}" >&2
  echo >&2
  echo "Everything else was reset. Re-running is safe and usually enough." >&2
  exit 1
fi

cat <<EOF

$repo is ready. To use it:

  export GH_SETTINGS_TEST_REPO=$repo
  mise run test:live

Or, to keep it across shells, in mise.local.toml (git-ignored):

  [env]
  GH_SETTINGS_TEST_REPO = "$repo"
EOF
