#!/usr/bin/env bash
# cofoundry-crawl installer — downloads binary + registers as Claude Code MCP server
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/twoLoop-40/cofoundry-crawl/main/install.sh | bash
#   # or with a specific version:
#   curl -fsSL ... | bash -s -- v0.2.0

set -euo pipefail

REPO="twoLoop-40/cofoundry-crawl"
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="cofoundry-crawl"
MCP_CONFIG="${HOME}/.claude/.mcp.json"

# ── Detect platform ──────────────────────────────────────────────────
detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "${os}" in
    Darwin)
      case "${arch}" in
        arm64|aarch64) echo "aarch64-apple-darwin" ;;
        x86_64)        echo "x86_64-apple-darwin" ;;
        *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;;
      esac
      ;;
    Linux)
      case "${arch}" in
        x86_64) echo "x86_64-unknown-linux-gnu" ;;
        *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;;
      esac
      ;;
    *) echo "Unsupported OS: ${os}" >&2; exit 1 ;;
  esac
}

# ── Get latest version ───────────────────────────────────────────────
get_version() {
  if [ -n "${1:-}" ]; then
    echo "$1"
    return
  fi
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 | sed -E 's/.*"([^"]+)".*/\1/'
}

# ── Main ─────────────────────────────────────────────────────────────
main() {
  local version platform asset_name url

  echo "🔧 cofoundry-crawl installer"
  echo ""

  platform="$(detect_platform)"
  version="$(get_version "${1:-}")"
  asset_name="${BINARY_NAME}-${platform}"
  url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"

  echo "  Platform: ${platform}"
  echo "  Version:  ${version}"
  echo "  URL:      ${url}"
  echo ""

  # Download
  mkdir -p "${INSTALL_DIR}"
  echo "📥 Downloading to ${INSTALL_DIR}/${BINARY_NAME}..."
  curl -fsSL -o "${INSTALL_DIR}/${BINARY_NAME}" "${url}"
  chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

  # Verify
  echo "✅ Installed: $(${INSTALL_DIR}/${BINARY_NAME} --version 2>/dev/null || echo "${INSTALL_DIR}/${BINARY_NAME}")"

  # Add to PATH if needed
  if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
    echo ""
    echo "⚠️  ${INSTALL_DIR} is not in your PATH. Add it:"
    echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.zshrc"
  fi

  # Register as Claude Code MCP server
  echo ""
  echo "🔌 Registering as Claude Code MCP server..."
  mkdir -p "$(dirname "${MCP_CONFIG}")"

  if [ -f "${MCP_CONFIG}" ]; then
    # Update existing config — add/replace cofoundry-crawl entry
    python3 -c "
import json, sys
with open('${MCP_CONFIG}') as f:
    config = json.load(f)
config.setdefault('mcpServers', {})
config['mcpServers']['cofoundry-crawl'] = {
    'command': '${INSTALL_DIR}/${BINARY_NAME}',
    'args': ['serve']
}
with open('${MCP_CONFIG}', 'w') as f:
    json.dump(config, f, indent=2)
print('   Updated ${MCP_CONFIG}')
" 2>/dev/null || {
      echo "   ⚠️ Could not update ${MCP_CONFIG} automatically."
      echo "   Add this manually:"
      echo '   {"mcpServers":{"cofoundry-crawl":{"command":"'"${INSTALL_DIR}/${BINARY_NAME}"'","args":["serve"]}}}'
    }
  else
    cat > "${MCP_CONFIG}" <<MCPEOF
{
  "mcpServers": {
    "cofoundry-crawl": {
      "command": "${INSTALL_DIR}/${BINARY_NAME}",
      "args": ["serve"]
    }
  }
}
MCPEOF
    echo "   Created ${MCP_CONFIG}"
  fi

  echo ""
  echo "🎉 Done! Restart Claude Code to use cofoundry-crawl MCP tools."
  echo "   Tools: login, crawl_url, screenshot, render_batch, search_site, extract_content"
}

main "$@"
