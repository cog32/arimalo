#!/bin/bash

# Copybara sync scripts for gleaned-covid repository
# Make sure you have Copybara installed: https://github.com/google/copybara

# Sync from public gleaned TO private gleaned-covid
sync_from_public() {
    echo "Syncing from public gleaned repo to private 'public' branch..."
    ./copybara copy.bara.sky public_to_private --force
}

# Sync from private gleaned-covid TO public gleaned (from 'public' branch)
# NOTE: Deprecated - use to-public-from-main instead
sync_to_public() {
    echo "⚠️  WARNING: Syncing from 'public' branch is deprecated."
    echo "    Use 'to-public-from-main' to sync directly from 'main' branch instead."
    echo ""
    echo "Syncing curated 'public' branch to public gleaned repo (opens PR)..."
    # Pass through any extra args to Copybara so callers can supply, e.g.,
    #   --labels=release_title:Release\ v1.2.3
    # or any other Copybara flags.
    ./copybara copy.bara.sky private_to_public --force "$@"
}

# Sync from main branch directly to public repo using Copybara
sync_to_public_from_main() {
    echo "Syncing from 'main' branch to public gleaned repo (opens PR)..."
    local current_branch=$(git rev-parse --abbrev-ref HEAD)

    if [ "$current_branch" != "main" ]; then
        echo "⚠️  WARNING: You are on branch '${current_branch}', not 'main'."
        echo "    Copybara will sync from 'main' branch."
    fi

    # Pass through any extra args to Copybara so callers can supply, e.g.,
    #   --labels=issue:25
    # or any other Copybara flags.
    ./copybara copy.bara.sky main_to_public --force "$@"
}

# Show help
show_help() {
    echo "Usage: $0 [COMMAND]"
    echo ""
    echo "Commands:"
    echo "  from-public          Update private 'public' branch from public gleaned"
    echo "  to-public-from-main  Open PR from 'main' to public gleaned (RECOMMENDED)"
    echo "  to-public            Open PR from 'public' to public gleaned (DEPRECATED)"
    echo "  help                 Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 from-public              # Pull latest changes from public repo"
    echo "  $0 to-public-from-main --labels=issue:25  # Push from main to public repo"
}

# Main script logic
case "$1" in
    "from-public")
        sync_from_public
        ;;
    "to-public")
        # Forward any extra args after 'to-public' to the copybara invocation
        # so you can pass flags like --labels=release_title:My\ title
        shift
        sync_to_public "$@"
        ;;
    "to-public-from-main")
        # Forward any extra args after 'to-public-from-main' to the copybara invocation
        shift
        sync_to_public_from_main "$@"
        ;;
    "help"|"--help"|"-h")
        show_help
        ;;
    *)
        echo "Error: Unknown command '$1'"
        echo ""
        show_help
        exit 1
        ;;
esac
