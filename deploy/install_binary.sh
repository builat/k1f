#!/usr/bin/env bash
#
# install_binary.sh — install an already-built k1f-tg binary as a systemd
# service on a Linux host.
#
# Usage:
#   sudo ./install_binary.sh /path/to/k1f-tg
#
# The binary MUST be built for the target Linux host (cargo build --release on
# the same distro/architecture, or cross-compiled accordingly).
#
# Run from anywhere — deploy/ files are resolved relative to this script.

set -euo pipefail

# --- pretty-ish output ------------------------------------------------------
c_red() { printf '\033[31m'; }
c_grn() { printf '\033[32m'; }
c_ylw() { printf '\033[33m'; }
c_off() { printf '\033[0m'; }

info() { printf '%s==>%s %s\n' "$(c_grn)" "$(c_off)" "$*"; }
warn() { printf '%s==>%s %s\n' "$(c_ylw)" "$(c_off)" "$*"; }
err()  { printf '%s!!%s %s\n'  "$(c_red)" "$(c_off)" "$*" >&2; }
die()  { err "$*"; exit 1; }

# Prompt the operator for the three secrets and write them into the env file.
# Tokens are read without echo. Existing lines are never overwritten: pressing
# Enter on a prompt keeps the template placeholder.
fill_env_file_interactive() {
    local file="$1"

    printf '\n'
    warn "Enter values. Press Enter to keep the existing placeholder."
    printf '\n'

    read -rp "TELOXIDE_TOKEN (from @BotFather): " tok </dev/tty
    read -rp "MASTER_TG_ID (your numeric Telegram id): " uid </dev/tty
    read -rsp "GPT_TOKEN (OpenAI key, hidden): " gpt </dev/tty; printf '\n'

    [[ -n "${tok}" ]] && sed -i "s|^TELOXIDE_TOKEN=.*|TELOXIDE_TOKEN=${tok}|" "${file}"
    [[ -n "${uid}" ]] && sed -i "s|^MASTER_TG_ID=.*|MASTER_TG_ID=${uid}|"     "${file}"
    [[ -n "${gpt}" ]] && sed -i "s|^GPT_TOKEN=.*|GPT_TOKEN=${gpt}|"           "${file}"
}

# --- sanity checks ----------------------------------------------------------
[[ "$(uname -s)" == "Linux" ]] || die "This installer targets Linux (got $(uname -s))."

if [[ $EUID -ne 0 ]]; then
    err "This installer must be run as root."
    die "Re-run with: sudo $0 $*"
fi

command -v systemctl >/dev/null 2>&1 || die "systemctl not found. systemd is required."

[[ $# -ge 1 ]] || die "Usage: sudo $0 <path-to-k1f-tg-binary>"
SRC_BIN="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
[[ -f "${SRC_BIN}" ]] || die "Binary not found: ${SRC_BIN}"
[[ -x "${SRC_BIN}" ]] || die "Binary not executable: ${SRC_BIN} (check it was built for this host)"

# Refuse a Windows/macOS-style executable to catch obvious cross-target mistakes.
file "${SRC_BIN}" | grep -qi 'elf' || warn "Binary doesn't look like a Linux ELF — if cross-compiling, double-check the target."

# Resolve the repo root: this script lives in <root>/deploy/.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="${SCRIPT_DIR}"

[[ -f "${DEPLOY_DIR}/k1f-tg.service" ]] || die "Missing ${DEPLOY_DIR}/k1f-tg.service — expected next to this script."
[[ -f "${DEPLOY_DIR}/k1f-tg.env.example" ]] || die "Missing ${DEPLOY_DIR}/k1f-tg.env.example."

# --- constants (must match k1f-tg.service) ----------------------------------
SERVICE_USER="k1f"
SERVICE_GROUP="k1f"
INSTALL_BIN="/usr/local/bin/k1f-tg"
WORK_DIR="/var/lib/k1f"
ENV_FILE="/etc/k1f-tg.env"
UNIT_DEST="/etc/systemd/system/k1f-tg.service"

# --- 1. system user + work dir ---------------------------------------------
if ! id "${SERVICE_USER}" >/dev/null 2>&1; then
    info "Creating system user '${SERVICE_USER}'..."
    useradd --system --no-create-home --shell /usr/sbin/nologin "${SERVICE_USER}"
else
    warn "User '${SERVICE_USER}' already exists — leaving as is."
fi
install -d -o "${SERVICE_USER}" -g "${SERVICE_GROUP}" -m 750 "${WORK_DIR}"

# --- 2. install binary ------------------------------------------------------
info "Installing binary ${SRC_BIN} -> ${INSTALL_BIN}..."
install -m 755 "${SRC_BIN}" "${INSTALL_BIN}"

# --- 3. env file (do not clobber existing secrets) --------------------------
if [[ -f "${ENV_FILE}" ]]; then
    warn "${ENV_FILE} already exists — keeping current secrets."
else
    info "Creating ${ENV_FILE} from template. Have your tokens ready."
    install -m 600 "${DEPLOY_DIR}/k1f-tg.env.example" "${ENV_FILE}"
    fill_env_file_interactive "${ENV_FILE}"
fi
chown root:root "${ENV_FILE}"; chmod 600 "${ENV_FILE}"

# --- 4. unit ----------------------------------------------------------------
info "Installing systemd unit to ${UNIT_DEST}..."
install -m 644 "${DEPLOY_DIR}/k1f-tg.service" "${UNIT_DEST}"

systemctl daemon-reload
systemctl enable --now k1f-tg

# --- done -------------------------------------------------------------------
printf '\n'
info "k1f-tg installed and started."
printf '  status:  %ssystemctl status k1f-tg%s\n' "$(c_grn)" "$(c_off)"
printf '  logs:    %sjournalctl -u k1f-tg -f%s\n'   "$(c_grn)" "$(c_off)"
printf '  env:     %s (chmod 600, root:root)\n' "${ENV_FILE}"
printf '  restart: %ssudo systemctl restart k1f-tg%s\n' "$(c_grn)" "$(c_off)"
