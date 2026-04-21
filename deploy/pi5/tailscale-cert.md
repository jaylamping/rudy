# Tailscale TLS for the Rudy operator console

`cortex` is reachable over the tailnet at **`https://<host>/`** (e.g.
`https://rudy-pi/`) — short MagicDNS name, no port, no `.ts.net` suffix.

There are two TLS surfaces; Tailscale handles both certs, but in different
ways:

| Surface                             | Listener           | TLS terminated by                     |
| ----------------------------------- | ------------------ | ------------------------------------- |
| REST API + SPA (`https://<host>/`)  | `127.0.0.1:8443`   | `tailscale serve` on `:443`           |
| Telemetry firehose (WebTransport)   | `<tailnet-ip>:4433`| `cortex` itself, via `wtransport`    |

`tailscale serve` is HTTP/1.1+HTTP/2 only — it cannot proxy HTTP/3 / QUIC,
which is what WebTransport uses. So WT keeps doing its own TLS with the same
Let's Encrypt cert that Tailscale issues.

See [ADR-0004 §D3](../../docs/decisions/0004-operator-console.md) and the
"Tailscale Serve" addendum for the rationale.

## What `bootstrap.sh` does for you

A fresh `bootstrap.sh` run, with Tailscale already up:

1. Issues a Let's Encrypt cert (via `tailscale cert`) into
   `/var/lib/rudy/cortex/tailscale/<host>.<tailnet>.ts.net.{crt,key}`. This is
   the cert the WebTransport listener loads.
2. Configures `tailscale serve --bg --https=443 http://127.0.0.1:8443`.
   Tailscale auto-renews the cert it uses for `:443` — you do nothing.
3. Renders `/etc/rudy/cortex.toml` with WT pointed at the cert files from
   step 1, and the HTTP listener bound to `127.0.0.1:8443` plaintext.
4. Starts `cortex.service`.

`apply-release.sh` re-runs steps 2 and 3 on every release so any cert-path
or Tailscale-identity drift heals itself.

## One-time prerequisites (only if `bootstrap.sh` warns)

```bash
# 1. Tailscale must be installed and logged in:
tailscale status

# 2. HTTPS Certificates must be enabled for the tailnet (one-time per tailnet):
#      https://login.tailscale.com/admin/dns -> "HTTPS Certificates" -> Enable

# 3. The cert directory must exist and be owned by the `rudy` user:
sudo install -d -o rudy -g rudy -m 0750 /var/lib/rudy/cortex/tailscale

# 4. Issue the cert (auto-renew handled later by a follow-up timer):
TAILNAME="$(hostname -s).$(tailscale status --json | jq -r '.MagicDNSSuffix')"
sudo tailscale cert \
  --cert-file "/var/lib/rudy/cortex/tailscale/${TAILNAME}.crt" \
  --key-file  "/var/lib/rudy/cortex/tailscale/${TAILNAME}.key" \
  "${TAILNAME}"
sudo chown rudy:rudy "/var/lib/rudy/cortex/tailscale/${TAILNAME}".*
sudo chmod 0640 "/var/lib/rudy/cortex/tailscale/${TAILNAME}.key"

# 5. Wire `tailscale serve` for the REST/SPA surface:
sudo tailscale serve --bg --https=443 http://127.0.0.1:8443

# 6. Re-render the daemon config and restart:
sudo bash /opt/rudy/deploy/pi5/render-cortex-toml.sh /etc/rudy/cortex.toml
sudo systemctl restart cortex
```

## Inspect / debug

```bash
tailscale serve status              # what is being proxied where
tailscale cert --help               # cert provisioning help
ls -la /var/lib/rudy/cortex/tailscale/    # cert files for WebTransport
ss -tlnp | grep -E '8443|4433|443'  # who is listening on what
journalctl -u cortex -f              # daemon logs
journalctl -u tailscaled -f         # Tailscale agent (incl. Serve activity)
```

## Renewal

- **REST/SPA cert (used by `tailscale serve`):** Tailscale renews this
  automatically on the daemon's schedule. No action.
- **WebTransport cert (`/var/lib/rudy/cortex/tailscale/<host>...`):** issued
  manually by step 4 above; expires after 90 days. To renew:

  ```bash
  TAILNAME="$(hostname -s).$(tailscale status --json | jq -r '.MagicDNSSuffix')"
  cd /var/lib/rudy/cortex/tailscale
  sudo tailscale cert \
    --cert-file "${TAILNAME}.crt" \
    --key-file  "${TAILNAME}.key" \
    "${TAILNAME}"
  sudo systemctl reload-or-restart cortex
  ```

  A systemd timer (`cortex-cert-renew.timer`) automates this in a future
  phase. Phase 1 does it by hand.

## Firewall

`tailscale serve` exposes `:443` only on `tailscale0`, so no firewall rule
is needed for the REST/SPA surface — Tailscale enforces tailnet-only on
your behalf.

WebTransport on `:4433/udp` *is* a regular UDP listener and must be
restricted to `tailscale0`. A working `nftables` fragment:

```
table inet cortex {
  chain input {
    type filter hook input priority 0;
    iif != "tailscale0" udp dport 4433 drop
  }
}
```

## Browser trust

Both surfaces use a real Let's Encrypt cert, so Chrome/Edge accept HTTPS
and WebTransport without any developer-mode flag or cert-fingerprint
pinning. No `chrome://flags` changes.
