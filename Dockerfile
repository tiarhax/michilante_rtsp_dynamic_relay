FROM ubuntu:24.10 AS chef
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav libgstrtspserver-1.0-dev libges-1.0-dev && \
    rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y libgstreamer-plugins-bad1.0-dev && \
    rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | bash -s -- -y 
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-chef 

WORKDIR app


FROM chef AS planner
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json


FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release


FROM ubuntu:24.10 AS runtime
WORKDIR app
RUN apt-get update && apt-get install -y \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav libgstrtspserver-1.0-dev libges-1.0-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

    
COPY --from=builder /app/target/release/dynamic-rtsp-relay /usr/local/bin
ENTRYPOINT ["/usr/local/bin/dynamic-rtsp-relay"]

