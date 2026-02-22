# List all available targets
default:
    @just --list

# Serve the documentation localy
[group: 'docs']
serve-docs:
    @uv run mkdocs serve

# Build the documentation
[group: 'docs']
build-docs:
    @uv run mkdocs build
