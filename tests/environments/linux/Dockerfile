FROM ubuntu:24.04@sha256:008173c23f95b170204355c12626cb5a965d779a7e1283b09e9cffbb1bf33ca3

# Disposable test server for the ssh-copy-id CLI integration tests. Not a product dependency.
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server tcsh \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /run/sshd \
    && ssh-keygen -A \
    && useradd --create-home --shell /bin/sh keyuser \
    && useradd --create-home --shell /bin/sh pwuser \
    && useradd --create-home --shell /usr/bin/tcsh cshuser \
    && useradd --create-home --shell /bin/sh emptyuser \
    && useradd --create-home --shell /bin/sh mfauser \
    && passwd -l keyuser \
    && passwd -l pwuser \
    && passwd -l cshuser \
    && passwd -l mfauser \
    && passwd -d emptyuser

COPY sshd_config /etc/ssh/sshd_config.d/fixture.conf
COPY emptyuser-banner /etc/ssh/emptyuser-banner
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
EXPOSE 22
ENTRYPOINT ["/bin/sh", "/usr/local/bin/entrypoint.sh"]
