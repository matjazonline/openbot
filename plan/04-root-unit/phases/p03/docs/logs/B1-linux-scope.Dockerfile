FROM ubuntu@sha256:73b999393d07f2bf65a40373778a44dc9a6d54ad7718d29247d8ff9413c4d6a8
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y python3 sudo iptables iproute2 procps passwd util-linux && rm -rf /var/lib/apt/lists/*
