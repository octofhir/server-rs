#!/bin/bash
# OctoFHIR Deployment Script
# This script automates the deployment of OctoFHIR on a Linux server

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
INSTALL_DIR="/opt/octofhir"
CONFIG_DIR="$INSTALL_DIR/config"
BIN_DIR="$INSTALL_DIR/bin"
DATA_DIR="$INSTALL_DIR/data"
LOGS_DIR="$INSTALL_DIR/logs"
DOCKER_DIR="$INSTALL_DIR/docker"
SCRIPTS_DIR="$INSTALL_DIR/scripts"
BACKUPS_DIR="$INSTALL_DIR/backups"

echo_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

echo_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_root() {
    if [ "$EUID" -ne 0 ]; then
        echo_error "This script must be run as root"
        exit 1
    fi
}

create_directories() {
    echo_info "Creating directory structure..."
    mkdir -p "$INSTALL_DIR"/{config,bin,data,logs,docker,scripts,backups}
    echo_info "Directories created"
}

check_dependencies() {
    echo_info "Checking dependencies..."

    if ! command -v docker &> /dev/null; then
        echo_error "Docker is not installed. Please install Docker first."
        exit 1
    fi

    if ! command -v docker compose &> /dev/null; then
        echo_error "Docker Compose is not installed. Please install Docker Compose plugin."
        exit 1
    fi

    echo_info "All dependencies satisfied"
}

deploy_postgres() {
    echo_info "Deploying PostgreSQL..."

    if [ ! -f "$DOCKER_DIR/docker-compose.yml" ]; then
        echo_error "docker-compose.yml not found in $DOCKER_DIR"
        echo_error "Please copy deployment/docker-compose.yml to $DOCKER_DIR"
        exit 1
    fi

    cd "$DOCKER_DIR"
    docker compose up -d postgres

    echo_info "Waiting for PostgreSQL to be ready..."
    for i in {1..30}; do
        if docker exec octofhir-postgres pg_isready -U postgres &> /dev/null; then
            echo_info "PostgreSQL is ready"
            return 0
        fi
        echo -n "."
        sleep 2
    done

    echo_error "PostgreSQL failed to start"
    docker compose logs postgres
    exit 1
}

install_binary() {
    echo_info "Installing OctoFHIR binary..."

    if [ ! -f "$BIN_DIR/octofhir-server" ]; then
        echo_error "Binary not found at $BIN_DIR/octofhir-server"
        echo_error "Please build and copy the binary first"
        exit 1
    fi

    chmod +x "$BIN_DIR/octofhir-server"
    echo_info "Binary installed and executable"
}

setup_config() {
    echo_info "Setting up configuration..."

    if [ ! -f "$CONFIG_DIR/octofhir.toml" ]; then
        echo_error "Configuration file not found at $CONFIG_DIR/octofhir.toml"
        echo_error "Please copy octofhir.toml to $CONFIG_DIR"
        exit 1
    fi

    chmod 600 "$CONFIG_DIR/octofhir.toml"
    echo_info "Configuration file secured"
}

install_service() {
    echo_info "Installing systemd service..."

    if [ ! -f "/etc/systemd/system/octofhir.service" ]; then
        echo_error "Service file not found at /etc/systemd/system/octofhir.service"
        echo_error "Please copy deployment/octofhir-root.service to /etc/systemd/system/octofhir.service"
        exit 1
    fi

    systemctl daemon-reload
    systemctl enable octofhir
    echo_info "Service installed and enabled"
}

start_service() {
    echo_info "Starting OctoFHIR service..."
    systemctl start octofhir

    sleep 5

    if systemctl is-active --quiet octofhir; then
        echo_info "Service started successfully"
    else
        echo_error "Service failed to start. Check logs with: journalctl -u octofhir -n 50"
        exit 1
    fi
}

verify_deployment() {
    echo_info "Verifying deployment..."

    # Check service status
    if ! systemctl is-active --quiet octofhir; then
        echo_error "Service is not running"
        return 1
    fi

    # Check if API responds
    sleep 5
    if curl -s http://localhost:8888/health &> /dev/null; then
        echo_info "API health check passed"
    else
        echo_warn "API health check failed - service may still be starting"
    fi

    echo_info "Deployment verification complete"
}

print_summary() {
    echo ""
    echo "================================================"
    echo_info "OctoFHIR Deployment Complete!"
    echo "================================================"
    echo ""
    echo "Service Status:"
    systemctl status octofhir --no-pager -l
    echo ""
    echo "Useful Commands:"
    echo "  - View logs:     journalctl -u octofhir -f"
    echo "  - Restart:       systemctl restart octofhir"
    echo "  - Stop:          systemctl stop octofhir"
    echo "  - Check status:  systemctl status octofhir"
    echo ""
    echo "API Endpoints:"
    echo "  - Health:        http://localhost:8888/health"
    echo "  - Metadata:      http://localhost:8888/fhir/metadata"
    echo "  - Web UI:        http://localhost:8888/"
    echo ""
    echo_warn "IMPORTANT: Change default passwords in $CONFIG_DIR/octofhir.toml"
    echo ""
}

# Main deployment flow
main() {
    echo_info "Starting OctoFHIR deployment..."

    check_root
    check_dependencies
    create_directories
    deploy_postgres
    install_binary
    setup_config
    install_service
    start_service
    verify_deployment
    print_summary
}

# Run main function
main
