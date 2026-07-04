# Running k1f-tg as a systemd service

This directory holds everything needed to run `k1f-tg` as a systemd service on
a Linux host.

| File                       | Purpose                                                |
| -------------------------- | ------------------------------------------------------ |
| `install_from_source.sh`   | one-shot installer: builds with cargo, then installs   |
| `install_binary.sh`        | one-shot installer for an already-built binary         |
| `k1f-tg.service`           | systemd unit, hardened but compatible with `/ping`     |
| `k1f-tg.env.example`       | template for the secrets file (`/etc/k1f-tg.env`)      |

## Quick start

Two installers cover the two ways of getting the binary onto the host. Both:
- must be run as **root** (`sudo`),
- are **idempotent** on the env file — existing `/etc/k1f-tg.env` is never
  overwritten, so re-running an installer to upgrade won't drop your secrets,
- ask interactively for `TELOXIDE_TOKEN` / `MASTER_TG_ID` / `GPT_TOKEN` the
  first time only (tokens are read without echo).

### Option A — build on the host (requires Rust toolchain)

```bash
git clone https://github.com/builat/k1f.git && cd k1f
sudo ./deploy/install_from_source.sh
```

Builds `target/release/k1f-tg` with `cargo build --release` and installs it.

### Option B — install a pre-built binary

Build on a machine with Rust, copy the binary and the repo's `deploy/` dir to
the target host, then:

```bash
sudo ./deploy/install_binary.sh /path/to/k1f-tg
```

The binary must be built for the target Linux distro/architecture.

## What an installer does, step by step

1. Verifies it's running on Linux, as root, with `systemctl` available
   (`install_from_source.sh` also checks for `cargo`).
2. Builds (A) or copies (B) the binary to `/usr/local/bin/k1f-tg`.
3. Creates a dedicated system user `k1f` and a writable work dir
   `/var/lib/k1f` (skipped if already present).
4. Writes `/etc/k1f-tg.env` from the template, prompting for secrets (only if
   the file doesn't already exist). Locks it down to `root:root` / `0600`.
5. Installs `k1f-tg.service`, runs `daemon-reload`, and `enable --now`s it.

## Day-to-day operations

```bash
# Status / "is it up?"
systemctl status k1f-tg

# Live logs (pretty_env_logger writes to stderr → journald)
journalctl -u k1f-tg -f

# Last 200 lines
journalctl -u k1f-tg -n 200 --no-pager

# Restart after re-running an installer with a new binary
sudo systemctl restart k1f-tg
```

## Notes on the hardening options

Most service-level restrictions are enabled (`ProtectSystem=strict`,
`PrivateDevices=true`, etc.). Two are deliberately **left off** so the `/ping`
command keeps working — the `pinger` crate spawns `/bin/ping`, which on most
distros relies on file capabilities:

- `NoNewPrivileges=true` would drop those capabilities and break `/ping`.
- `RestrictAddressFamilies=` is not tightened, because the spawned `ping`
  process needs to create raw/ICMP sockets.

If you do not need `/ping`, you can set both for a tighter sandbox.
