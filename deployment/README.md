# OctoFHIR Deployment Files

This directory contains everything needed to deploy OctoFHIR on a Linux production server.

## 📁 Directory Structure

```
deployment/
├── README.md                    # This file
├── QUICKSTART.md               # Fast track deployment guide (5 minutes)
├── DEPLOYMENT.md               # Comprehensive deployment documentation
├── docker-compose.yml          # PostgreSQL + Redis Docker setup
├── octofhir.service            # Systemd service (dedicated user - recommended)
├── octofhir-root.service       # Systemd service (root user - not recommended)
├── octofhir-production.toml    # Production configuration template
├── postgres-init/              # PostgreSQL initialization scripts
│   └── 01-performance-tuning.sql
└── scripts/
    └── deploy.sh               # Automated deployment script
```

## 🚀 Quick Start

Choose your deployment path:

### 1. Fast Track (5 minutes)

For quick testing or simple deployments:

```bash
# Read the quick start guide
cat QUICKSTART.md
```

**Follow:** [QUICKSTART.md](./QUICKSTART.md)

### 2. Comprehensive Setup (Recommended for Production)

For production deployments with all best practices:

```bash
# Read the full deployment guide
cat DEPLOYMENT.md
```

**Follow:** [DEPLOYMENT.md](./DEPLOYMENT.md)

### 3. Automated Deployment

Use the deployment script for automated setup:

```bash
# 1. Prepare files on your server
mkdir -p /opt/octofhir/{config,bin,docker}

# 2. Copy required files
cp deployment/docker-compose.yml /opt/octofhir/docker/
cp deployment/octofhir-production.toml /opt/octofhir/config/octofhir.toml
cp deployment/octofhir-root.service /etc/systemd/system/octofhir.service
cp target/release/octofhir-server /opt/octofhir/bin/

# 3. Run deployment script
bash deployment/scripts/deploy.sh
```

## 📋 Deployment Checklist

- [ ] Install Docker and Docker Compose
- [ ] Create directory structure (`/opt/octofhir`)
- [ ] Build or transfer binary to server
- [ ] Copy configuration files
- [ ] Start PostgreSQL with docker-compose
- [ ] Configure octofhir.toml (passwords, domain, JWT keys)
- [ ] Install systemd service
- [ ] Start and enable service
- [ ] Verify deployment (health check, API test)
- [ ] Set up reverse proxy with SSL (Nginx + Let's Encrypt)
- [ ] Configure firewall rules
- [ ] Set up automated backups
- [ ] Monitor logs and performance

## 🔒 Security Checklist

**IMPORTANT:** Complete these before going to production:

- [ ] Change all default passwords in config
- [ ] Generate production JWT signing keys
- [ ] Set correct `issuer` URL in config
- [ ] Run as dedicated user (not root)
- [ ] Set up SSL/TLS (HTTPS)
- [ ] Configure firewall (UFW/iptables)
- [ ] Enable automated backups
- [ ] Set up monitoring and alerting
- [ ] Disable GraphQL introspection in production
- [ ] Use read-only mode for DB console in production
- [ ] Regular security updates

## 📦 What's Included

### Docker Compose

- **PostgreSQL 16**: Main database with performance tuning
- **Redis** (optional): For caching in multi-instance deployments
- Persistent volumes for data
- Health checks
- Automatic restarts

### Systemd Services

Two service file options:

1. **octofhir.service**: Runs as dedicated `octofhir` user (recommended)
2. **octofhir-root.service**: Runs as root (simpler but less secure)

### Configuration

- **octofhir-production.toml**: Production-ready config template
- Includes all necessary settings
- Comments explain each option
- Ready for environment variable overrides

### Scripts

- **deploy.sh**: Automated deployment script
- **backup-db.sh**: Database backup script (created during setup)

## 🏗️ Architecture

```
┌─────────────────┐
│   Nginx/Caddy   │  (Reverse proxy with SSL)
│   Port 80/443   │
└────────┬────────┘
         │
┌────────▼────────┐
│  OctoFHIR Server│  (Systemd service)
│   Port 8888     │
└────────┬────────┘
         │
         ├──────────────┐
         │              │
┌────────▼────────┐ ┌──▼──────┐
│   PostgreSQL    │ │  Redis  │  (Optional)
│  Port 5450      │ │ Port 6380│
│  (Docker)       │ │(Docker) │
└─────────────────┘ └─────────┘
```

## 📊 Resource Requirements

### Minimum (Testing)
- 2 CPU cores
- 2 GB RAM
- 20 GB disk

### Recommended (Production)
- 4+ CPU cores
- 8+ GB RAM
- 100+ GB SSD
- Postgres: 256MB shared_buffers, 1GB cache

### High Performance
- 8+ CPU cores
- 16+ GB RAM
- 500+ GB NVMe SSD
- Postgres: 512MB shared_buffers, 2GB cache
- Redis enabled
- Multiple server instances with load balancer

## 🔧 Configuration Override

You can override any config setting with environment variables:

```bash
# In systemd service file
Environment="OCTOFHIR__SERVER__PORT=8080"
Environment="OCTOFHIR__STORAGE__POSTGRES__HOST=db.internal"
Environment="OCTOFHIR__AUTH__ISSUER=https://fhir.your-domain.com"
```

## 📝 Common Tasks

### Update Server

```bash
# Build new binary
cargo build --release

# On server:
systemctl stop octofhir
cp /path/to/new/binary /opt/octofhir/bin/octofhir-server
systemctl start octofhir
journalctl -u octofhir -f
```

### View Logs

```bash
# Real-time logs
journalctl -u octofhir -f

# Last 100 lines
journalctl -u octofhir -n 100

# Logs since boot
journalctl -u octofhir -b

# Logs with specific priority
journalctl -u octofhir -p err
```

### Database Maintenance

```bash
# Backup
docker exec octofhir-postgres pg_dump -U postgres octofhir | gzip > backup.sql.gz

# Restore
gunzip -c backup.sql.gz | docker exec -i octofhir-postgres psql -U postgres -d octofhir

# Vacuum
docker exec octofhir-postgres psql -U postgres -d octofhir -c "VACUUM ANALYZE;"

# Check size
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT pg_size_pretty(pg_database_size('octofhir'));"
```

## 🐛 Troubleshooting

### Issue: Service won't start

```bash
journalctl -u octofhir -n 50 --no-pager
systemctl status octofhir -l
```

### Issue: Can't connect to database

```bash
docker ps | grep postgres
docker logs octofhir-postgres
docker exec octofhir-postgres psql -U postgres -d octofhir -c "SELECT 1;"
```

### Issue: High memory usage

```bash
# Check resources
htop
docker stats

# Adjust PostgreSQL pool size in config
nano /opt/octofhir/config/octofhir.toml
# [storage.postgres]
# pool_size = 20  # Reduce if needed
```

## 📖 Additional Resources

- Main README: `../README.md`
- Configuration Reference: `../octofhir.toml`
- API Documentation: `http://your-server:8888/fhir/metadata`
- GraphQL Console: `http://your-server:8888/ui/graphql`

## 🆘 Support

- GitHub Issues: https://github.com/octofhir/octofhir/issues
- Documentation: Check main README.md
- Logs: `journalctl -u octofhir -f`

---

**Ready to deploy?** Start with [QUICKSTART.md](./QUICKSTART.md)!
