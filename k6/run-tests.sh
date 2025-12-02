#!/bin/bash
# k6 Test Runner for OctoFHIR Server
# Usage: ./k6/run-tests.sh [test-type] [scenario]
#
# Examples:
#   ./k6/run-tests.sh crud              # Run CRUD functional tests
#   ./k6/run-tests.sh validation        # Run validation tests
#   ./k6/run-tests.sh performance smoke # Run smoke performance test
#   ./k6/run-tests.sh performance load  # Run load test
#   ./k6/run-tests.sh all               # Run all tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_URL="${BASE_URL:-http://localhost:8888}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "\n${BLUE}════════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}\n"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_info() {
    echo -e "${YELLOW}→ $1${NC}"
}

# Check if k6 is installed
check_k6() {
    if ! command -v k6 &> /dev/null; then
        print_error "k6 is not installed"
        echo "Install k6:"
        echo "  macOS: brew install k6"
        echo "  Linux: https://k6.io/docs/getting-started/installation/"
        exit 1
    fi
    print_success "k6 is installed: $(k6 version)"
}

# Check if server is reachable
check_server() {
    print_info "Checking server at $BASE_URL..."
    if curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/healthz" | grep -q "200"; then
        print_success "Server is reachable"
    else
        print_error "Server is not reachable at $BASE_URL"
        echo "Make sure the OctoFHIR server is running:"
        echo "  cargo run --release"
        exit 1
    fi
}

# Create results directory
mkdir -p "$SCRIPT_DIR/results"

# Run CRUD tests
run_crud() {
    print_header "Running Patient CRUD Functional Tests"
    k6 run --env BASE_URL="$BASE_URL" "$SCRIPT_DIR/tests/patient-crud.js"
}

# Run validation tests
run_validation() {
    print_header "Running Patient Validation Tests"
    k6 run --env BASE_URL="$BASE_URL" "$SCRIPT_DIR/tests/patient-validation.js"
}

# Run performance tests
run_performance() {
    local scenario="${1:-smoke}"
    print_header "Running Patient Performance Tests (Scenario: $scenario)"
    k6 run --env BASE_URL="$BASE_URL" --env SCENARIO="$scenario" "$SCRIPT_DIR/tests/patient-performance.js"
}

# Run all tests
run_all() {
    print_header "Running All Tests"

    print_info "Step 1/3: CRUD Tests"
    run_crud

    print_info "Step 2/3: Validation Tests"
    run_validation

    print_info "Step 3/3: Performance Tests (smoke)"
    run_performance smoke

    print_success "All tests completed!"
}

# Main
main() {
    print_header "OctoFHIR k6 Test Runner"

    check_k6
    check_server

    case "${1:-help}" in
        crud)
            run_crud
            ;;
        validation)
            run_validation
            ;;
        performance)
            run_performance "${2:-smoke}"
            ;;
        all)
            run_all
            ;;
        help|*)
            echo "Usage: $0 [test-type] [options]"
            echo ""
            echo "Test types:"
            echo "  crud              Run CRUD functional tests"
            echo "  validation        Run validation/negative tests"
            echo "  performance [s]   Run performance tests with scenario [s]"
            echo "  all               Run all tests"
            echo ""
            echo "Performance scenarios:"
            echo "  smoke             Quick sanity check (1 VU, 5 iterations)"
            echo "  performance       Performance baseline (10 VUs, 1 minute)"
            echo "  load              Load test (ramp to 20 VUs)"
            echo "  stress            Stress test (ramp to 100 VUs)"
            echo "  spike             Spike test (sudden 100 VU spike)"
            echo ""
            echo "Environment variables:"
            echo "  BASE_URL          Server URL (default: http://localhost:8888)"
            echo ""
            echo "Examples:"
            echo "  $0 crud"
            echo "  $0 performance load"
            echo "  BASE_URL=http://prod.example.com $0 performance smoke"
            ;;
    esac
}

main "$@"
