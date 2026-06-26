#!/bin/sh
set -eu

REPO="${TMUSSH_REPO:-thisdotrob/tmussh}"
REMOTE="${TMUSSH_RELEASE_REMOTE:-origin}"
BASE_BRANCH="${TMUSSH_RELEASE_BASE:-main}"
MERGE_RELEASE="${TMUSSH_RELEASE_MERGE:-1}"
RUN_CHECKS="${TMUSSH_RELEASE_CHECKS:-1}"

fail() {
  printf 'tmussh release: %s\n' "$1" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

usage() {
  cat <<EOF
Usage: scripts/release.sh minor|major

When no vX.Y.Z tags exist, publishes the current Cargo.toml version as the
initial release. Otherwise, creates a release branch, bumps
Cargo.toml/Cargo.lock, opens a PR, and pushes a vX.Y.Z tag after the PR is
merged. Pushing the tag triggers the GitHub release workflow.

Environment:
  TMUSSH_RELEASE_MERGE=0    Open the release PR without merging/tagging it.
                            For the initial release, print tag commands only.
  TMUSSH_RELEASE_CHECKS=0   Skip local cargo checks before committing.
EOF
}

current_version() {
  awk '
    $0 == "[package]" { in_package = 1; next }
    /^\[/ { in_package = 0 }
    in_package && $1 == "version" {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$@"
}

validate_version() {
  version=$1

  old_ifs=$IFS
  IFS=.
  set -- $version
  IFS=$old_ifs

  [ "$#" -eq 3 ] || fail "Cargo.toml version must be MAJOR.MINOR.PATCH"

  case "$version" in
    *[!0-9.]* | *..* | .* | *.) fail "Cargo.toml version must be MAJOR.MINOR.PATCH" ;;
  esac
}

next_version() {
  bump=$1
  version=$2

  validate_version "$version"

  old_ifs=$IFS
  IFS=.
  set -- $version
  IFS=$old_ifs

  major=$1
  minor=$2
  patch=$3

  case "$bump" in
    major) printf '%s.0.0\n' "$((major + 1))" ;;
    minor) printf '%s.%s.0\n' "$major" "$((minor + 1))" ;;
    *) fail "release type must be minor or major" ;;
  esac
}

set_cargo_version() {
  version=$1
  tmp=$(mktemp "${TMPDIR:-/tmp}/tmussh-cargo.XXXXXXXXXX")

  awk -v version="$version" '
    $0 == "[package]" { in_package = 1; print; next }
    /^\[/ && $0 != "[package]" { in_package = 0 }
    in_package && $1 == "version" && !done {
      print "version = \"" version "\""
      done = 1
      next
    }
    { print }
    END { if (!done) exit 1 }
  ' Cargo.toml > "$tmp" || {
    rm -f "$tmp"
    fail "failed to update Cargo.toml version"
  }

  mv "$tmp" Cargo.toml
}

ensure_clean_worktree() {
  [ -z "$(git status --porcelain)" ] || fail "working tree must be clean"
}

ensure_tag_available() {
  tag=$1

  if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    fail "tag $tag already exists locally"
  fi

  if git ls-remote --exit-code --tags "$REMOTE" "refs/tags/$tag" >/dev/null 2>&1; then
    fail "tag $tag already exists on $REMOTE"
  fi
}

ensure_branch_available() {
  branch=$1

  if git show-ref --verify --quiet "refs/heads/$branch"; then
    fail "branch $branch already exists locally"
  fi

  if git ls-remote --exit-code --heads "$REMOTE" "$branch" >/dev/null 2>&1; then
    fail "branch $branch already exists on $REMOTE"
  fi
}

remote_release_tags() {
  git ls-remote --tags "$REMOTE" 'refs/tags/v[0-9]*.[0-9]*.[0-9]*'
}

run_local_checks() {
  if [ "$RUN_CHECKS" = "0" ]; then
    return
  fi

  if ls scripts/*.sh >/dev/null 2>&1; then
    for script in scripts/*.sh; do
      sh -n "$script"
    done
  fi

  cargo fmt --check
  cargo test
  cargo clippy --all-targets -- -D warnings
}

publish_tag() {
  tag=$1

  git fetch "$REMOTE" "$BASE_BRANCH" --tags --prune
  ensure_tag_available "$tag"
  git tag -a "$tag" -m "$tag" "$REMOTE/$BASE_BRANCH"
  git push "$REMOTE" "$tag"
}

[ "$#" -eq 1 ] || {
  usage
  exit 1
}

case "$1" in
  -h | --help)
    usage
    exit 0
    ;;
  minor | major) bump=$1 ;;
  *)
    usage
    exit 1
    ;;
esac

need awk
need cargo
need gh
need git
need mktemp

ensure_clean_worktree

git fetch "$REMOTE" "$BASE_BRANCH" --tags --prune
version=$(git show "$REMOTE/$BASE_BRANCH:Cargo.toml" | current_version)
release_tags=$(remote_release_tags) || fail "failed to list release tags from $REMOTE"

if [ -n "$release_tags" ]; then
  next=$(next_version "$bump" "$version")
  initial_release=0
else
  validate_version "$version"
  next=$version
  initial_release=1
fi

tag="v$next"
branch="release/$tag"

ensure_tag_available "$tag"

if [ "$initial_release" = "1" ]; then
  if [ "$MERGE_RELEASE" = "0" ]; then
    cat <<EOF
No existing release tags were found. Cargo.toml is already at $next, so no
release PR is needed. Publish the initial release with:

  git fetch $REMOTE $BASE_BRANCH --tags --prune
  git tag -a $tag -m $tag $REMOTE/$BASE_BRANCH
  git push $REMOTE $tag
EOF
    exit 0
  fi

  publish_tag "$tag"
  printf 'Released %s. The GitHub release workflow will publish assets.\n' "$tag"
  exit 0
fi

ensure_branch_available "$branch"

git switch -c "$branch" "$REMOTE/$BASE_BRANCH"
set_cargo_version "$next"
cargo check
run_local_checks

git add Cargo.toml Cargo.lock
git commit -m "Release $tag"
git push -u "$REMOTE" "$branch"

pr_url=$(gh pr create \
  --repo "$REPO" \
  --base "$BASE_BRANCH" \
  --head "$branch" \
  --title "Release $tag" \
  --body "Bump tmussh to $tag and publish release assets after merge.")
pr_number=${pr_url##*/}

printf 'Created release PR: %s\n' "$pr_url"

if [ "$MERGE_RELEASE" = "0" ]; then
  cat <<EOF
Merge the PR, then publish the release with:

  git fetch $REMOTE $BASE_BRANCH --tags --prune
  git tag -a $tag -m $tag $REMOTE/$BASE_BRANCH
  git push $REMOTE $tag
EOF
  exit 0
fi

if gh pr merge "$pr_number" --repo "$REPO" --rebase --delete-branch; then
  publish_tag "$tag"
  printf 'Released %s. The GitHub release workflow will publish assets.\n' "$tag"
else
  cat <<EOF
The release PR was created but could not be merged automatically.
After it is merged, publish the release with:

  git fetch $REMOTE $BASE_BRANCH --tags --prune
  git tag -a $tag -m $tag $REMOTE/$BASE_BRANCH
  git push $REMOTE $tag
EOF
fi
