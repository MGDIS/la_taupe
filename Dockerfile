# Multi-stage build for La Taupe
# Stage 1: Builder - Compile the Rust application
FROM rust:1.92-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock build.rs ./

COPY src ./src

RUN mkdir -p models && \
    curl -L "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten" \
    -o models/text-detection.rten && \
    curl -L "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten" \
    -o models/text-recognition.rten && \
    echo "Models downloaded successfully"

RUN cargo build --release

# Stage 2: Runtime - Create minimal runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
# - poppler-utils: for pdftotext (PDF text extraction)
# - tesseract-ocr: OCR engine
# - tesseract-ocr-fra: French language data for Tesseract
# - ca-certificates: for HTTPS requests
RUN apt-get update && apt upgrade -y && apt-get install -y \
    poppler-utils \
    tesseract-ocr \
    tesseract-ocr-fra \
    ca-certificates \
    htop \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r taupe && useradd -r -g taupe taupe

WORKDIR /app

COPY --from=builder /app/target/release/la_taupe /usr/local/bin/la_taupe

COPY --from=builder /app/models /app/models

RUN chown -R taupe:taupe /app

USER taupe

EXPOSE 8080

ENV RUST_LOG=info
ENV LA_TAUPE_ADDRESS=0.0.0.0:8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/ping || exit 1

CMD ["la_taupe"]
