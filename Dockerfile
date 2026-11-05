# Multi-stage build for La Taupe
# Stage 1: Builder - Compile the Rust application
FROM rust:1.97.1-trixie AS builder

RUN apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y \
    pkg-config \
    libssl-dev \
    libtesseract-dev \
    libleptonica-dev \
    libclang-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml download-models.sh ./

RUN mkdir -p models && bash download-models.sh

COPY src ./src

RUN cargo build --release

# Stage 2: Runtime - Create minimal runtime image
FROM debian:trixie-slim

# Install runtime dependencies
# - poppler-utils: for pdftotext (PDF text extraction)
# - tesseract-ocr: OCR engine
# - tesseract-ocr-fra: French language data for Tesseract
# - ca-certificates: for HTTPS requests
RUN apt-get -o Acquire::Retries=5 update && apt upgrade -y && apt-get -o Acquire::Retries=5 install -y \
    poppler-utils \
    tesseract-ocr \
    tesseract-ocr-fra \
    ca-certificates \
    htop \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r taupe && useradd -r -g taupe taupe

WORKDIR /app

COPY --from=builder /app/target/release/la_taupe /usr/local/bin/la_taupe

RUN chown -R taupe:taupe /app

USER taupe

EXPOSE 8080

ENV RUST_LOG=info
ENV LA_TAUPE_ADDRESS=0.0.0.0:8080

CMD ["la_taupe"]
