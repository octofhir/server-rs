# OctoFHIR Quick Deployment Guide

Fast track deployment for production Linux servers.

## Prerequisites

- Linux server with root access
- Docker and Docker Compose installed

## Quick Install (5 Minutes)

### 1. Install Docker (if not already installed)

```bash
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh
apt install docker-compose-plugin -y
```

### 2. Create Installation Directory

```bash
mkdir -p /opt/octofhir/{config,bin,docker,data,logs,scripts,backups}
```

### 3. Build and Transfer Binary

**On your development machine:**

```bash
# Build release binary
cd /path/to/octofhir/server-rs
cargo build --release

# Transfer to server
scp target/release/octofhir-server root@YOUR_SERVER:/opt/octofhir/bin/
scp deployment/docker-compose.yml root@YOUR_SERVER:/opt/octofhir/docker/
scp deployment/octofhir-production.toml root@YOUR_SERVER:/opt/octofhir/config/octofhir.toml
scp deployment/octofhir-root.service root@YOUR_SERVER:/etc/systemd/system/octofhir.service
scp -r deployment/postgres-init root@YOUR_SERVER:/opt/octofhir/docker/
```

### 4. Deploy on Server

```bash
# Make binary executable
chmod +x /opt/octofhir/bin/octofhir-server

# Start PostgreSQL
cd /opt/octofhir/docker
docker compose up -d postgres

# Wait for PostgreSQL (about 10 seconds)
sleep 10

# Verify PostgreSQL is running
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT version();"

# Install and start systemd service
systemctl daemon-reload
systemctl enable octofhir
systemctl start octofhir

# Check status
systemctl status octofhir
```

### 5. Verify Installation

```bash
# Check health
curl http://localhost:8888/health

# Check FHIR metadata
curl http://localhost:8888/fhir/metadata

# Create test patient
curl -X POST http://localhost:8888/fhir/Patient \
  -H "Content-Type: application/fhir+json" \
  -d '{"resourceType":"Patient","name":[{"family":"Doe","given":["John"]}]}'
```

### 6. Access Web UI

Open in your browser:
- Main UI: `http://YOUR_SERVER_IP:8888/`
- GraphQL Console: `http://YOUR_SERVER_IP:8888/ui/graphql`
- DB Console: `http://YOUR_SERVER_IP:8888/ui/db-console`

**Default credentials:**
- Username: `admin`
- Password: `admin123` (or check your config file)

## Post-Installation

### ⚠️ Security Checklist

**IMPORTANT:** Complete these steps immediately after installation:

```bash
# 1. Edit configuration
nano /opt/octofhir/config/octofhir.toml

# Change these settings:
# - [auth] issuer = "https://your-actual-domain.com"
# - [bootstrap.admin_user] password = "YOUR_STRONG_PASSWORD"
# - [storage.postgres] password = "YOUR_DB_PASSWORD"

# 2. Generate production JWT keys
cd /opt/octofhir/config
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-384 -out private.pem
openssl ec -in private.pem -pubout -out public.pem
chmod 600 private.pem public.pem

# Copy the key contents into octofhir.toml under [auth.signing]

# 3. Restart service
systemctl restart octofhir

# 4. Set up firewall
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw enable
```

### Set Up Reverse Proxy (Production)

For production with SSL:

```bash
# Install Nginx
apt install nginx certbot python3-certbot-nginx -y

# Create nginx config
cat > /etc/nginx/sites-available/octofhir << 'EOF'
server {
    listen 80;
    server_name your-domain.com;
    client_max_body_size 10M;

    location / {
        proxy_pass http://localhost:8888;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
EOF

ln -s /etc/nginx/sites-available/octofhir /etc/nginx/sites-enabled/
nginx -t
systemctl reload nginx

# Install SSL certificate
certbot --nginx -d your-domain.com
```

### Set Up Automated Backups

```bash
# Create backup script
cat > /opt/octofhir/scripts/backup-db.sh << 'EOF'
#!/bin/bash
BACKUP_DIR="/opt/octofhir/backups"
DATE=$(date +%Y%m%d_%H%M%S)
mkdir -p $BACKUP_DIR
docker exec octofhir-postgres pg_dump -U postgres octofhir | gzip > "$BACKUP_DIR/octofhir_$DATE.sql.gz"
find $BACKUP_DIR -name "octofhir_*.sql.gz" -mtime +30 -delete
echo "Backup completed: octofhir_$DATE.sql.gz"
EOF

chmod +x /opt/octofhir/scripts/backup-db.sh

# Schedule daily backups at 2 AM
(crontab -l 2>/dev/null; echo "0 2 * * * /opt/octofhir/scripts/backup-db.sh") | crontab -
```

## Common Commands

```bash
# Service Management
systemctl status octofhir      # Check status
systemctl restart octofhir     # Restart
systemctl stop octofhir        # Stop
systemctl start octofhir       # Start
journalctl -u octofhir -f      # View logs

# Docker Management
cd /opt/octofhir/docker
docker compose ps              # Check containers
docker compose logs postgres   # PostgreSQL logs
docker compose restart         # Restart all containers

# Database Access
docker exec -it octofhir-postgres psql -U postgres -d octofhir

# Backup & Restore
/opt/octofhir/scripts/backup-db.sh  # Manual backup
gunzip -c /opt/octofhir/backups/octofhir_*.sql.gz | \
  docker exec -i octofhir-postgres psql -U postgres -d octofhir  # Restore
```

## Troubleshooting

### Service won't start

```bash
# Check logs
journalctl -u octofhir -n 100 --no-pager

# Check if PostgreSQL is running
docker ps | grep postgres

# Test database connection
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT 1;"
```

### Can't connect to API

```bash
# Check if service is running
systemctl status octofhir

# Check if port is listening
netstat -tlnp | grep 8888

# Check firewall
ufw status
```

### Database issues

```bash
# View PostgreSQL logs
docker logs octofhir-postgres

# Restart PostgreSQL
cd /opt/octofhir/docker
docker compose restart postgres
```

## Need More Details?

See [DEPLOYMENT.md](./DEPLOYMENT.md) for comprehensive documentation.

---

**Deployment Complete!** 🎉

Your OctoFHIR server is now running at:
- Local: `http://localhost:8888`
- External: `http://YOUR_SERVER_IP:8888`

Don't forget to:
1. ✅ Change default passwords
2. ✅ Generate production JWT keys
3. ✅ Set up SSL with reverse proxy
4. ✅ Configure automated backups
5. ✅ Set up firewall rules
