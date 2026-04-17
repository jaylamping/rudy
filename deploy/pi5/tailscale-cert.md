# Tailscale HTTPS for rudyd

`rudyd` serves HTTPS (for the REST API + SPA) and WebTransport over HTTP/3
(for the telemetry firehose). Both listeners require a trusted TLS
certificate; we use Tailscale's built-in HTTPS to provision a real Let's
Encrypt cert, avoiding self-signed cert glue.

See [ADR-0004 section D3](../../docs/decisions/0004-operator-console.md) for
why.

## One-time setup

```bash
# On the Pi, as root:

# 1. Confirm Tailscale is up and the Pi has a stable MagicDNS name.
tailscale status
tailscale cert --help     # must not error

# 2. Enable HTTPS for the tailnet if you have not already:
#    https://login.tailscale.com/admin/dns -> "HTTPS Certificates"
#    (Toggle it on. One-time, per tailnet.)

# 3. Create the cert directory owned by the `rudy` user.
install -d -o rudy -g rudy -m 0750 /var/lib/rudyd/tailscale

# 4. Provision the cert. TAILNAME is your MagicDNS name (lowercase), e.g.
#    rudy.tail-scale.ts.net.
TAILNAME="rudy.$(tailscale status --json | jq -r '.MagicDNSSuffix')"
cd /var/lib/rudyd/tailscale
sudo tailscale cert "${TAILNAME}"
# Produces ${TAILNAME}.crt and ${TAILNAME}.key in CWD.
chown rudy:rudy ${TAILNAME}.*
chmod 0640 ${TAILNAME}.key

# 5. Point rudyd.toml at them.
#    [http.tls]
#    enabled = true
#    cert_path = "/var/lib/rudyd/tailscale/rudy.tail-scale.ts.net.crt"
#    key_path  = "/var/lib/rudyd/tailscale/rudy.tail-scale.ts.net.key"
#
#    [http]
#    bind = "100.x.y.z:8443"    # Tailscale address ONLY
#
#    [webtransport]
#    enabled = true
#    bind    = "100.x.y.z:4433"

sudo systemctl restart rudyd.service
```

## Renewal

Tailscale certs expire after 90 days. Renew in the same spot:

```bash
cd /var/lib/rudyd/tailscale
sudo tailscale cert "${TAILNAME}"
sudo systemctl reload-or-restart rudyd.service
```

A systemd timer (`rudyd-cert-renew.timer`) automates this in a future phase.
Phase 1 does it by hand.

## Firewall

`rudyd` must be reachable ONLY over Tailscale. The Pi's firewall should block
`:8443` and `:4433/udp` on every interface other than `tailscale0`. A
working `nftables` fragment:

```
table inet rudyd {
  chain input {
    type filter hook input priority 0;
    iif != "tailscale0" tcp dport 8443 drop
    iif != "tailscale0" udp dport 4433 drop
  }
}
```

## Browser trust

Because the cert is a real Let's Encrypt cert, Chrome/Edge accept HTTPS and
WebTransport without any developer-mode flag or cert-fingerprint pinning.
No `chrome://flags` changes.
