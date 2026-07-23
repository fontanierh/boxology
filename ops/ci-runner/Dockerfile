FROM --platform=linux/arm64 ubuntu:24.04@sha256:7f622ca8766bccb22f04242ecb6f19f770b2f08827dc4b8c707de5e78a6da7ab

ENV DEBIAN_FRONTEND=noninteractive \
    ImageOS=ubuntu24.04-arm64-colima \
    ImageVersion=ubuntu-24.04-arm64-runner-2.336.0-rust-1.97.1-deny-0.20.2-rustup-volume-1 \
    RUSTUP_HOME=/runner/_work/.rustup \
    CARGO_HOME=/runner/_work/.cargo \
    PATH=/runner/_work/.cargo/bin:/runner/_work/.rustup/toolchains/1.97.1-aarch64-unknown-linux-gnu/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
LABEL org.boxology.ci.image-id="boxology-linux-arm64-pr" \
      org.boxology.ci.image-version="ubuntu-24.04-arm64-runner-2.336.0-rust-1.97.1-deny-0.20.2-rustup-volume-1" \
      org.boxology.ci.base-digest="sha256:7f622ca8766bccb22f04242ecb6f19f770b2f08827dc4b8c707de5e78a6da7ab" \
      org.boxology.ci.runner-sha256="58b758e420b87093fbd4bfddd368074960053e2f1388f01848c82624b90f27d1"

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git jq build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --shell /bin/bash runner
WORKDIR /opt/actions-runner
RUN curl --fail --location --proto '=https' --tlsv1.2 \
      -o /tmp/actions-runner.tgz \
      "https://github.com/actions/runner/releases/download/v2.336.0/actions-runner-linux-arm64-2.336.0.tar.gz" \
    && echo "58b758e420b87093fbd4bfddd368074960053e2f1388f01848c82624b90f27d1  /tmp/actions-runner.tgz" | sha256sum -c - \
    && tar -xzf /tmp/actions-runner.tgz \
    && rm /tmp/actions-runner.tgz \
    && ./bin/installdependencies.sh
RUN mkdir -p /runner/_work/.rustup /runner/_work/.cargo \
    && touch /runner/.boxology-rustup-volume-seed \
    && chown -R runner:runner /opt/actions-runner /runner
USER runner
RUN curl --fail --location --proto '=https' --tlsv1.2 https://sh.rustup.rs | sh -s -- -y --default-toolchain none \
    && rustup toolchain install 1.97.1 --profile minimal --component rustfmt --component clippy \
    && rustup default 1.97.1 \
    && cargo install cargo-deny --version 0.20.2 --locked
COPY --chown=runner:runner ops/ci-runner/entrypoint.sh /usr/local/bin/boxology-runner-entrypoint
RUN chmod 0755 /usr/local/bin/boxology-runner-entrypoint
ENTRYPOINT ["/usr/local/bin/boxology-runner-entrypoint"]
