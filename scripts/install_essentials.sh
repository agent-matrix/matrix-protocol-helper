#!/usr/bin/env bash

# --- install_essentials.sh ---
# A script to install essential packages and build tools on a new Linux system.
# It automatically detects the distribution and uses the appropriate package manager.

set -euo pipefail # Exit on error, undefined variable, or pipe failure.

# --- Cosmetics & Logging ---
# Check if stdout is a terminal to enable/disable colors
if [[ -t 1 ]]; then
  readonly C_INFO='\033[0;34m'
  readonly C_OK='\033[0;32m'
  readonly C_WARN='\033[0;33m'
  readonly C_ERR='\033[0;31m'
  readonly C_NC='\033[0m' # No Color
else
  readonly C_INFO='' C_OK='' C_WARN='' C_ERR='' C_NC=''
fi

say() {
  local color="$1"
  local message="$2"
  echo -e "${color}▶ ${message}${C_NC}"
}

# --- Root Check ---
# Ensure the script is run with root privileges (e.g., using sudo).
if [[ "$(id -u)" -ne 0 ]]; then
  say "$C_ERR" "This script must be run as root. Please use 'sudo'."
  exit 1
fi

# --- Package Lists ---
# Add or remove packages here as needed.
readonly COMMON_PACKAGES=(
  git
  curl
  wget
  htop
  tmux
  neofetch
  unzip
  zip
  p7zip-full
  net-tools
  dnsutils
  python3-pip
  python3-venv
  vim
  ca-certificates
)

main() {
  say "$C_INFO" "Starting the essential package installer..."

  # --- Distro Detection ---
  local os_id
  if [[ -f /etc/os-release ]]; then
    # freedesktop.org and systemd
    . /etc/os-release
    os_id=$ID
  else
    say "$C_ERR" "Cannot determine Linux distribution."
    exit 1
  fi

  local install_cmd update_cmd packages
  packages=("${COMMON_PACKAGES[@]}") # Start with the common list

  say "$C_INFO" "Detected Distribution: $os_id"

  case "$os_id" in
    ubuntu|debian|pop)
      update_cmd="apt-get update"
      install_cmd="apt-get install -y"
      packages+=(build-essential libssl-dev apt-transport-https software-properties-common libsoup-3.0-dev libwebkit2gtk-4.1-dev libfuse2)
      ;;
    fedora|centos|rhel)
      update_cmd="dnf check-update"
      install_cmd="dnf install -y"
      # The '@' installs the entire "Development Tools" group
      packages+=('@Development Tools' 'openssl-devel')
      ;;
    arch|manjaro)
      # Arch updates and installs in one command with -Syu
      update_cmd="" # Not needed, handled by install
      install_cmd="pacman -Syu --noconfirm"
      packages+=(base-devel openssl)
      ;;
    *)
      say "$C_ERR" "Unsupported distribution: '$os_id'. Exiting."
      exit 1
      ;;
  esac

  # --- Confirmation ---
  say "$C_WARN" "This script will install the following packages:"
  echo "  ${packages[*]}"
  echo ""
  read -p "Do you want to proceed? (y/N) " -r REPLY
  if [[ ! "$REPLY" =~ ^[Yy]$ ]]; then
    say "$C_WARN" "Installation cancelled by user."
    exit 0
  fi

  # --- Installation ---
  say "$C_INFO" "Updating package lists..."
  if [[ -n "$update_cmd" ]]; then
    $update_cmd
  fi
  
  say "$C_INFO" "Installing essential packages..."
  # shellcheck disable=SC2068
  $install_cmd ${packages[@]}

  say "$C_OK" "✅ Essential packages have been installed successfully!"
}

# Run the main function
main