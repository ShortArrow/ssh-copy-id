# OpenWrt 24.10.8, x86/64 root file system: BusyBox ash and Dropbear as on a router.
FROM openwrt/rootfs:x86-64-24.10.8@sha256:9972a4b4747cd136abd597475d7b88c51a49fd849d0d53f069a2f4bf446061b9

# Disposable test server for the ssh-copy-id CLI integration tests (L03). Not a product dependency.
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
EXPOSE 22
ENTRYPOINT ["/bin/sh", "/usr/local/bin/entrypoint.sh"]
