# Container image used to validate Claude Code settings against their JSON schema.
FROM python:3.14-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends jq \
    && rm -rf /var/lib/apt/lists/*

ARG CHECK_JSONSCHEMA_VERSION=0.39.0
RUN pip install --no-cache-dir "check-jsonschema==${CHECK_JSONSCHEMA_VERSION}"

COPY validate.sh /usr/local/bin/validate-claude
RUN chmod +x /usr/local/bin/validate-claude

WORKDIR /workspace
ENTRYPOINT ["validate-claude"]
