FROM node:22-slim AS builder
WORKDIR /build
COPY app/package*.json ./
RUN npm ci --no-audit --no-fund
COPY app/ ./
RUN npm run build

FROM python:3.12-slim
WORKDIR /app
COPY server.py .
COPY --from=builder /build/dist dashboard/
CMD ["python3", "server.py"]
