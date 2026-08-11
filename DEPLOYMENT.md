# Deployment

Production runbook for StellarGate on [Fly.io](https://fly.io). The same image
runs anywhere Docker does; only the platform commands differ.

- [Before you deploy](#before-you-deploy)
- [First deploy](#first-deploy)
- [Provisioning a merchant](#provisioning-a-merchant)
- [Operating](#operating)
- [Upgrades and rollback](#upgrades-and-rollback)
- [Backups](#backups)
- [Scaling limits](#scaling-limits)
- [Other platforms](#other-platforms)

---

## Before you deploy

### 1. A funded Stellar account

The gateway watches one account for incoming payments. It **never holds the
secret key** — set only the public key.

```bash
# Testnet: create and fund an account at https://laboratory.stellar.org
# Mainnet: use an account you control. Fund it with the 1 XLM base reserve
# plus 0.5 XLM per trustline.
```

Add a trustline for every non-native asset you intend to accept (USDC and so
on). A payment in an asset with no trustline **will fail on-chain**, so the
gateway logs a warning at startup listing any accepted asset that is missing
one. Check the logs after your first deploy.

### 2. Generate real secrets

Never reuse the values from `.env.example` — startup rejects known
placeholders, and `WEBHOOK_SECRET` must be at least 32 characters.

```bash
openssl rand -hex 32   # WEBHOOK_SECRET
openssl rand -hex 32   # ADMIN_PROVISIONING_SECRET
```

### 3. Pre-flight checklist

| Check | Why |
|---|---|
| `STELLAR_NETWORK=public` | Otherwise you are watching testnet and will never see real payments |
| `STELLAR_HORIZON_URL` points at mainnet Horizon | Must match the network |
| `WEBHOOK_SECRET` ≥ 32 random chars | Signs every webhook; merchants verify against it |
| `ADMIN_PROVISIONING_SECRET` set | Unset disables merchant provisioning entirely |
| `CORS_ALLOWED_ORIGINS` set | **Required** on `public`; a missing value is a startup error |
| Trustlines added for every accepted asset | Payments in an untrusted asset bounce |
| `WEBHOOK_ALLOW_PRIVATE_TARGETS` unset/false | Enabling it in production reopens the SSRF hole |

---

## First deploy

```bash
# 0. Install flyctl and sign in
curl -L https://fly.io/install.sh | sh
fly auth login

# 1. Claim the app name. Edit `app` in fly.toml first — names are global.
fly launch --no-deploy --copy-config

# 2. Create the volume SQLite writes to. Must be in the same region as the app.
fly volumes create stellargate_data --size 1 --region lhr

# 3. Set secrets (never commit these; fly.toml is in git)
fly secrets set \
  WEBHOOK_SECRET="$(openssl rand -hex 32)" \
  ADMIN_PROVISIONING_SECRET="$(openssl rand -hex 32)" \
  STELLAR_GATEWAY_PUBLIC="GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX" \
  CORS_ALLOWED_ORIGINS="https://yourapp.example.com"

# 4. Ship it
fly deploy

# 5. Confirm it came up healthy
fly status
curl https://<your-app>.fly.dev/health   # {"status":"ok"}
curl https://<your-app>.fly.dev/ready    # {"status":"ok"} once Horizon is reachable
```

`/ready` returning `503` with `"reason":"..."` is the fastest way to tell
whether the database or Horizon is the problem.

### Custom domain

```bash
fly certs add pay.example.com   # then add the DNS records it prints
```

Once the domain is live, add it to `CORS_ALLOWED_ORIGINS`.

---

## Provisioning a merchant

There is no self-service signup. Mint keys yourself:

```bash
curl -X POST https://<your-app>.fly.dev/merchants \
  -H "X-Admin-Secret: $ADMIN_PROVISIONING_SECRET"
# {"merchant_id":"...","api_key":"..."}
```

The API key is shown **once** — only a hash is stored, so it cannot be
recovered. Hand it to the merchant over a secure channel. They then use it as
`Authorization: Bearer <key>`, and to sign in to the dashboard at
`/dashboard`.

---

## Operating

```bash
fly logs                      # stream structured logs
fly status                    # machine + health check state
fly ssh console               # shell into the running machine
fly dashboard                 # metrics and machine management
```

**Metrics.** `GET /metrics` exposes Prometheus counters for webhook delivery
outcomes, retries, delivery latency, and auth successes/failures. Point your
scraper at it.

**Alerts worth wiring up first:**

| Signal | Why it matters |
|---|---|
| `/ready` failing | Horizon or the database is unreachable — payments will not be detected |
| `stellargate_webhook_deliveries_total{outcome="failed"}` rising | Merchants are not being notified of completed payments |
| `cursor_age_secs` climbing in logs | The poller is falling behind the chain |
| `stellargate_auth_attempts_total{outcome="failure"}` spiking | Credential stuffing, or a broken integration |

**Exposure.** `/dashboard` leaks nothing without a valid API key, but there is
no reason to serve the sign-in page to the open internet. Restrict it at your
edge, or via `fly proxy` for operator-only access.

---

## Upgrades and rollback

Migrations in `migrations/` run automatically at startup and are recorded in
`_sqlx_migrations`, so each runs exactly once. Deploying a build with a new
migration applies it as the machine boots.

```bash
fly deploy                    # rolling deploy; health checks gate the cutover
fly releases                  # list past releases
fly deploy --image <previous> # roll back to a prior image
```

> **Rolling back across a migration does not undo it.** SQLite migrations here
> are forward-only. If a release adds a migration, take a backup first (below)
> and treat rollback as restore-from-backup rather than a redeploy.

---

## Backups

The entire dataset is one SQLite file on the volume. Fly snapshots volumes
daily by default, but take your own before anything risky:

```bash
# Consistent copy of a live SQLite database — do NOT just `cp` the file.
fly ssh console -C "sqlite3 /data/stellargate.db \".backup '/data/backup.db'\""
fly ssh sftp get /data/backup.db ./stellargate-$(date +%F).db
```

Restore by uploading the file back to `/data/stellargate.db` with the app
stopped.

> Copying `stellargate.db` directly while the app is running can capture a torn
> write, because WAL mode keeps recent commits in a side file. Use `.backup`,
> which takes a consistent snapshot.

---

## Scaling limits

Read this before scaling out — it is the sharpest constraint in the system.

**Run exactly one machine.** SQLite allows a single writer, and the volume is
local to one machine. Two machines would each hold their own database file and
each run their own Horizon listener and expiry sweeper — a payment could settle
twice and fire duplicate webhooks. `fly.toml` therefore pins
`min_machines_running = 1` with autoscaling off.

This is comfortable for a large volume of payments: the workload is a handful
of small writes per payment. What it does not survive is a machine failure —
you get the restart window as downtime.

To go multi-node you would need to move off SQLite to a networked database and
elect a single leader for the background listeners. That is a real project, not
a config change; the sqlx queries and migrations are SQLite-specific today.

**Vertical scaling** is the supported lever:

```bash
fly scale vm shared-cpu-2x --memory 1024
fly volumes extend <volume-id> --size 5
```

---

## Other platforms

The image is a plain, non-root Docker container with no platform coupling:

- **Any VPS / Docker host** — `docker compose up -d`, using the committed
  `docker-compose.yml` and a named volume.
- **Render** — Docker environment, health check path `/ready`, persistent disk
  mounted at `/data`.
- **Kubernetes / ECS** — one replica, `RollingUpdate` with `maxSurge: 0` so two
  instances never share the volume, `/health` as liveness and `/ready` as
  readiness.

Whatever the platform, three things must hold: **one instance**, a **persistent
volume at `/data`**, and secrets supplied as environment variables rather than
baked into the image.
