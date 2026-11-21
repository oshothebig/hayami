FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config clang libclang-dev curl ca-certificates git libgtk-4-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev xvfb fonts-noto-cjk fonts-dejavu ibus ibus-gtk4 ibus-mozc dbus-x11 xdotool imagemagick && rm -rf /var/lib/apt/lists/*
