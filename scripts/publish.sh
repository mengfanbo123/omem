#!/bin/bash
set -e

PUBLIC_REPO="https://x-access-token:${GH_TOKEN}@github.com/ourmem/omem.git"
TEMP_DIR=$(mktemp -d)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "📦 Syncing public files to ourmem/omem..."

git clone --depth 1 "$PUBLIC_REPO" "$TEMP_DIR" 2>/dev/null || git init "$TEMP_DIR"

rm -rf "$TEMP_DIR/plugins" "$TEMP_DIR/skills" "$TEMP_DIR/docs" "$TEMP_DIR/eval" "$TEMP_DIR/.cargo"

cp -r "$PROJECT_DIR/plugins" "$TEMP_DIR/plugins"
cp -r "$PROJECT_DIR/skills" "$TEMP_DIR/skills"
cp -r "$PROJECT_DIR/docs" "$TEMP_DIR/docs"
cp -r "$PROJECT_DIR/eval" "$TEMP_DIR/eval" 2>/dev/null || true
cp -r "$PROJECT_DIR/.cargo" "$TEMP_DIR/.cargo"

cp "$PROJECT_DIR/README.md" "$TEMP_DIR/"
cp "$PROJECT_DIR/README_CN.md" "$TEMP_DIR/"
cp "$PROJECT_DIR/LICENSE" "$TEMP_DIR/"
cp "$PROJECT_DIR/.env.example" "$TEMP_DIR/"
cp "$PROJECT_DIR/Makefile" "$TEMP_DIR/" 2>/dev/null || true
cp "$PROJECT_DIR/Dockerfile" "$TEMP_DIR/" 2>/dev/null || true
cp "$PROJECT_DIR/docker-compose.yml" "$TEMP_DIR/" 2>/dev/null || true
cp "$PROJECT_DIR/docker-compose.prod.yml" "$TEMP_DIR/" 2>/dev/null || true

# 公开仓库的 .gitignore（排除服务端代码）
cat > "$TEMP_DIR/.gitignore" << 'PUBIGNORE'
node_modules/
dist/
*.tsbuildinfo
bun.lockb
bun.lock
package-lock.json
.DS_Store
*.swp
*~
__pycache__/
*.pyc
target/
omem-data/
.env
.env.*
!.env.example
.sisyphus/
PUBIGNORE

find "$TEMP_DIR" -name "node_modules" -type d -exec rm -rf {} + 2>/dev/null || true

cd "$TEMP_DIR"
git add -A
CHANGES=$(git status --porcelain | wc -l)
if [ "$CHANGES" -eq 0 ]; then
    echo "✅ No changes to publish."
else
    LATEST_MSG=$(cd "$PROJECT_DIR" && git log -1 --pretty=format:"%s")
    git -c user.name="ourmem" -c user.email="ourmem@users.noreply.github.com" \
        commit -m "$LATEST_MSG"
    git push origin main 2>&1
    echo "✅ Published $CHANGES files to ourmem/omem"
fi

rm -rf "$TEMP_DIR"
