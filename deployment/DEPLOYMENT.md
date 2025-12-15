# OctoFHIR Linux Deployment Guide

Complete step-by-step instructions for deploying OctoFHIR server on a Linux machine with PostgreSQL in Docker.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [PostgreSQL Setup (Docker)](#postgresql-setup-docker)
3. [Server Build & Installation](#server-build--installation)
4. [Systemd Service Setup](#systemd-service-setup)
5. [Configuration](#configuration)
6. [Deployment](#deployment)
7. [Verification](#verification)
8. [Maintenance](#maintenance)
9. [Troubleshooting](#troubleshooting)

---

## Prerequisites

### System Requirements

- Linux server (Ubuntu 20.04+, Debian 11+, RHEL 8+, or similar)
- Minimum 2 GB RAM (4 GB+ recommended)
- 20 GB disk space
- Root access

### Install Required Software

```bash
# Update system
apt update && apt upgrade -y

# Install Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh

# Install Docker Compose
apt install docker-compose-plugin -y

# Verify Docker installation
docker --version
docker compose version

# Enable Docker to start on boot
systemctl enable docker
systemctl start docker

# Install essential build tools (if building on the server)
apt install -y build-essential pkg-config libssl-dev
```

---

## PostgreSQL Setup (Docker)

### 1. Create Deployment Directory

```bash
# Create directory structure
mkdir -p /opt/octofhir/{config,data,logs,docker}

# Copy docker-compose.yml to the docker directory
cd /opt/octofhir/docker
```

### 2. Create docker-compose.yml

Copy the provided `deployment/docker-compose.yml` file to `/opt/octofhir/docker/docker-compose.yml`.

Or create it directly:

```bash
cat > /opt/octofhir/docker/docker-compose.yml << 'EOF'
version: '3.8'

services:
  postgres:
    image: postgres:16-alpine
    container_name: octofhir-postgres
    restart: unless-stopped
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: octofhir
      POSTGRES_SHARED_BUFFERS: 256MB
      POSTGRES_EFFECTIVE_CACHE_SIZE: 1GB
      POSTGRES_MAINTENANCE_WORK_MEM: 64MB
      POSTGRES_WAL_BUFFERS: 8MB
    ports:
      - "5450:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks:
      - octofhir

volumes:
  postgres_data:
    driver: local

networks:
  octofhir:
    driver: bridge
EOF
```

### 3. Start PostgreSQL

```bash
cd /opt/octofhir/docker

# Start PostgreSQL
docker compose up -d postgres

# Verify PostgreSQL is running
docker compose ps
docker compose logs postgres

# Test database connection
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT version();"
```

### 4. (Optional) Enable Redis for Caching

If you plan to run multiple OctoFHIR instances, enable Redis:

```bash
# Start with Redis profile
docker compose --profile with-redis up -d
```

---

## Server Build & Installation

### Option A: Build on the Server

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Clone the repository
cd /tmp
git clone https://github.com/your-org/octofhir.git
cd octofhir/server-rs

# Build release binary
cargo build --release

# Copy binary to installation directory
cp target/release/octofhir-server /opt/octofhir/bin/
chmod +x /opt/octofhir/bin/octofhir-server

# Copy default configuration
cp octofhir.toml /opt/octofhir/config/
```

### Option B: Build Locally and Transfer

On your local machine:

```bash
# Build release binary
cargo build --release

# Transfer to server
scp target/release/octofhir-server root@your-server:/opt/octofhir/bin/
scp octofhir.toml root@your-server:/opt/octofhir/config/
```

On the server:

```bash
chmod +x /opt/octofhir/bin/octofhir-server
```

---

## Systemd Service Setup

### 1. Copy Service File

Copy the provided `deployment/octofhir-root.service` file:

```bash
cp deployment/octofhir-root.service /etc/systemd/system/octofhir.service
```

Or create it directly:

```bash
cat > /etc/systemd/system/octofhir.service << 'EOF'
[Unit]
Description=OctoFHIR Server - High-performance FHIR R4B Server
Documentation=https://github.com/octofhir/octofhir
After=network-online.target docker.service
Wants=network-online.target
Requires=docker.service

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=/opt/octofhir

ExecStart=/opt/octofhir/bin/octofhir-server

Environment="RUST_LOG=info"
Environment="OCTOFHIR_CONFIG=/opt/octofhir/config/octofhir.toml"

Restart=always
RestartSec=10
StartLimitBurst=5
StartLimitIntervalSec=60

LimitNOFILE=65536
LimitNPROC=4096

StandardOutput=journal
StandardError=journal
SyslogIdentifier=octofhir

KillMode=mixed
KillSignal=SIGTERM

[Install]
WantedBy=multi-user.target
EOF
```

### 2. Reload Systemd

```bash
systemctl daemon-reload
```

---

## Configuration

### 1. Edit Production Configuration

```bash
cd /opt/octofhir/config

# Edit the configuration file
nano octofhir.toml
```

**Key settings to update:**

```toml
[server]
host = "0.0.0.0"
port = 8888

[storage.postgres]
host = "localhost"
port = 5450
user = "postgres"
password = "postgres"  # Change in production!
database = "octofhir"

[auth]
issuer = "https://your-domain.com"  # IMPORTANT: Set your actual domain

[bootstrap.admin_user]
username = "admin"
password = "CHANGE_ME_IMMEDIATELY"  # IMPORTANT: Change this!
email = "admin@your-domain.com"
```

### 2. Generate Production JWT Keys (Recommended)

For production, generate your own JWT signing keys:

```bash
# For ES384 (Elliptic Curve - recommended)
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-384 -out /opt/octofhir/config/private.pem
openssl ec -in /opt/octofhir/config/private.pem -pubout -out /opt/octofhir/config/public.pem

# Display keys to copy into config
echo "Private key:"
cat /opt/octofhir/config/private.pem
echo ""
echo "Public key:"
cat /opt/octofhir/config/public.pem
```

Copy the key contents (including BEGIN/END markers) into your `octofhir.toml` under `[auth.signing]`.

### 3. Set Proper Permissions

```bash
chmod 600 /opt/octofhir/config/octofhir.toml
chmod 600 /opt/octofhir/config/*.pem
```

---

## Deployment

### 1. Enable and Start the Service

```bash
# Enable service to start on boot
systemctl enable octofhir

# Start the service
systemctl start octofhir

# Check status
systemctl status octofhir
```

### 2. View Logs

```bash
# Follow logs in real-time
journalctl -u octofhir -f

# View last 100 lines
journalctl -u octofhir -n 100

# View logs since last boot
journalctl -u octofhir -b
```

---

## Verification

### 1. Check Service Status

```bash
systemctl status octofhir
```

Expected output:
```
● octofhir.service - OctoFHIR Server - High-performance FHIR R4B Server
     Loaded: loaded (/etc/systemd/system/octofhir.service; enabled; vendor preset: enabled)
     Active: active (running) since ...
```

### 2. Test API Endpoints

```bash
# Check metadata endpoint
curl http://localhost:8888/fhir/metadata

# Check health
curl http://localhost:8888/health

# Test creating a Patient resource
curl -X POST http://localhost:8888/fhir/Patient \
  -H "Content-Type: application/fhir+json" \
  -d '{
    "resourceType": "Patient",
    "name": [{"family": "Test", "given": ["John"]}]
  }'
```

### 3. Test Authentication

```bash
# Get admin token (use credentials from config)
curl -X POST http://localhost:8888/oauth/token \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=password&username=admin&password=admin123"
```

### 4. Access Web Console

Open in browser:
- Main UI: `http://your-server-ip:8888/`
- GraphQL Console: `http://your-server-ip:8888/ui/graphql`
- DB Console: `http://your-server-ip:8888/ui/db-console`

---

## Maintenance

### Service Management

```bash
# Stop service
systemctl stop octofhir

# Restart service
systemctl restart octofhir

# Check status
systemctl status octofhir

# Disable service (prevent auto-start)
systemctl disable octofhir
```

### Log Management

```bash
# Rotate logs (configure logrotate)
cat > /etc/logrotate.d/octofhir << 'EOF'
/opt/octofhir/logs/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    create 644 root root
}
EOF
```

### Database Backups

```bash
# Create backup script
cat > /opt/octofhir/scripts/backup-db.sh << 'EOF'
#!/bin/bash
BACKUP_DIR="/opt/octofhir/backups"
DATE=$(date +%Y%m%d_%H%M%S)
mkdir -p $BACKUP_DIR

docker exec octofhir-postgres pg_dump -U postgres octofhir | gzip > "$BACKUP_DIR/octofhir_$DATE.sql.gz"

# Keep only last 30 days of backups
find $BACKUP_DIR -name "octofhir_*.sql.gz" -mtime +30 -delete

echo "Backup completed: octofhir_$DATE.sql.gz"
EOF

chmod +x /opt/octofhir/scripts/backup-db.sh

# Add to crontab for daily backups
(crontab -l 2>/dev/null; echo "0 2 * * * /opt/octofhir/scripts/backup-db.sh") | crontab -
```

### Restore from Backup

```bash
# List backups
ls -lh /opt/octofhir/backups/

# Restore
gunzip -c /opt/octofhir/backups/octofhir_YYYYMMDD_HHMMSS.sql.gz | \
  docker exec -i octofhir-postgres psql -U postgres -d octofhir
```

### Updates

```bash
# Stop service
systemctl stop octofhir

# Backup current binary
cp /opt/octofhir/bin/octofhir-server /opt/octofhir/bin/octofhir-server.backup

# Replace with new binary
cp /path/to/new/octofhir-server /opt/octofhir/bin/
chmod +x /opt/octofhir/bin/octofhir-server

# Start service
systemctl start octofhir

# Check logs
journalctl -u octofhir -f
```

---

## Troubleshooting

### Service Won't Start

```bash
# Check service status
systemctl status octofhir -l

# Check logs
journalctl -u octofhir -n 100 --no-pager

# Check if port is in use
netstat -tlnp | grep 8888

# Check if PostgreSQL is running
docker ps | grep postgres
docker compose -f /opt/octofhir/docker/docker-compose.yml ps
```

### Database Connection Issues

```bash
# Test PostgreSQL connection
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT 1;"

# Check PostgreSQL logs
docker logs octofhir-postgres

# Verify network connectivity
telnet localhost 5450

# Check config file
grep -A 10 "\[storage.postgres\]" /opt/octofhir/config/octofhir.toml
```

### Performance Issues

```bash
# Check resource usage
htop

# Check database connections
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT count(*) FROM pg_stat_activity;"

# Check slow queries
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT query, calls, mean_exec_time FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 10;"

# Increase pool size in config if needed
nano /opt/octofhir/config/octofhir.toml
# [storage.postgres]
# pool_size = 50  # Increase if needed
```

### Permission Issues

```bash
# Fix permissions
chown -R root:root /opt/octofhir
chmod 755 /opt/octofhir/bin/octofhir-server
chmod 600 /opt/octofhir/config/octofhir.toml
```

### Reset Admin Password

```bash
# Connect to database
docker exec -it octofhir-postgres psql -U postgres -d octofhir

# List users
SELECT id, username, email FROM users;

# Update admin password (hashed - you'll need to restart server to regenerate)
# Or delete admin user and restart server to recreate:
DELETE FROM users WHERE username = 'admin';
\q

# Restart service to recreate admin user
systemctl restart octofhir
```

---

## Security Recommendations

⚠️ **IMPORTANT SECURITY NOTES:**

1. **Don't run as root in production**: Create a dedicated user:
   ```bash
   useradd -r -s /bin/false -d /opt/octofhir octofhir
   chown -R octofhir:octofhir /opt/octofhir
   # Edit /etc/systemd/system/octofhir.service and change User=octofhir
   ```

2. **Change default passwords** in `octofhir.toml`:
   - Database password
   - Admin user password
   - Generate production JWT keys

3. **Use HTTPS**: Set up a reverse proxy (nginx/caddy) with SSL:
   ```bash
   apt install nginx certbot python3-certbot-nginx
   certbot --nginx -d your-domain.com
   ```

4. **Firewall**: Configure firewall to only allow necessary ports:
   ```bash
   ufw allow 22/tcp    # SSH
   ufw allow 80/tcp    # HTTP
   ufw allow 443/tcp   # HTTPS
   ufw enable
   ```

5. **Regular updates**: Keep system and Docker images updated
6. **Monitoring**: Set up monitoring (Prometheus, Grafana) for production
7. **Backups**: Automate database backups (see Maintenance section)

---

## Reverse Proxy Setup (Production)

### Nginx Configuration

```bash
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

# Install SSL with Let's Encrypt
certbot --nginx -d your-domain.com
```

---

## Support

For issues and questions:
- GitHub Issues: https://github.com/octofhir/octofhir/issues
- Documentation: Check the main README.md

---

**Deployment Complete!** 🚀

Your OctoFHIR server should now be running at `http://localhost:8888`
