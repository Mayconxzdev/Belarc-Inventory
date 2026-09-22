# Belarc Inventory

Belarc Inventory is a Windows workstation inventory, health, audit, and department-based ticketing system for private LAN/VPN deployments.

Built by **Maycon Ferreira** and **Diogo Rodrigues**, it combines a Rust/PowerShell agent, Axum + SQLite server, IT dashboard, and workstation ticket portal.

This public repository contains source code, a reproducible local demo, documentation, and automated checks. It intentionally excludes corporate data, executable releases, credentials, inventory databases, internal network addresses, and production infrastructure.

## Local demo

```powershell
cargo build -p belarc-server -p belarc-agent
.\scripts\qa\isolated-smoke-test.ps1 -Build
```

See the Portuguese [README](README.md), [LAN/VPN deployment guide](docs/deployment-lan.md), and [security policy](SECURITY.md) for current limits and setup guidance.
