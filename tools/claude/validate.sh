#!/usr/bin/env bash
# Validate a Claude Code settings file against the schema referenced by its "$schema" field.
set -euo pipefail

settings="${1:-.claude/settings.json}"
schema_url="$(jq -er '."$schema"' "$settings")"
check-jsonschema --schemafile "$schema_url" "$settings"
