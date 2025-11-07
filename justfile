# List all available targets
default:
    @just --list

# Run all checks
check:
    bash check.sh

# Serve the documentation localy
[group: 'docs']
serve-docs:
    @uv run mkdocs serve

# Build the documentation
[group: 'docs']
build-docs:
    @uv run mkdocs build
