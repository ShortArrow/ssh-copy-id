FROM ubuntu:24.04@sha256:008173c23f95b170204355c12626cb5a965d779a7e1283b09e9cffbb1bf33ca3

# Explicit dependencies for this disposable test environment, not the product.
RUN apt-get update && apt-get install -y --no-install-recommends \
    openssh-client openssh-server python3 curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/sh fixture \
    && passwd -d fixture && mkdir -p /run/sshd

# Declared upstream test input, pinned to the design's comparison baseline.
RUN curl --fail --location --retry 3 \
    https://raw.githubusercontent.com/openssh/openssh-portable/eabf1987de772f0f2d772fd6bd72b4c84d0ab780/contrib/ssh-copy-id \
    --output /opt/upstream-ssh-copy-id \
    && echo 'a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650  /opt/upstream-ssh-copy-id' | sha256sum --check

COPY probe.py /opt/probe.py
CMD ["python3", "/opt/probe.py"]
