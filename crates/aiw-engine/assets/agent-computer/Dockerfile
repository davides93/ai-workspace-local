# The agent's computer, first piece: a browser the agent drives and the person can watch (Chrome DevTools Protocol on 9222).
# Built on the person's computer by `aiw containers up <agent>`; nothing is pulled from an AIW registry.
# NOTE: the base image is pinned by tag, not yet by digest (the digest could not be verified when this was written).
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends chromium fonts-liberation fonts-noto-color-emoji ca-certificates tini socat \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --uid 1000 --create-home --shell /usr/sbin/nologin agent

# The profile (cookies, logins) lives in the agent's own workspace, so it survives a restart and stays out of the image.
ENV HOME=/tmp
USER 1000:1000
WORKDIR /workspace
EXPOSE 9222
ENTRYPOINT ["/usr/bin/tini", "--"]
# --no-sandbox: Chromium's own sandbox needs privileges the container does not have (all capabilities are dropped); the
# container is the boundary here. The new headless mode ignores --remote-debugging-address and listens on 127.0.0.1 only,
# so socat re-exposes it on 9222 (the port is published on the host's loopback only, see compose.yml).
CMD ["sh", "-c", "socat TCP-LISTEN:9222,fork,reuseaddr,bind=0.0.0.0 TCP:127.0.0.1:9223 & exec chromium --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage --remote-debugging-port=9223 --user-data-dir=/workspace/.browser-profile --no-first-run --no-default-browser-check about:blank"]
