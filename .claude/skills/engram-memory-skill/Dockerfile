FROM python:3.12-slim

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt fastapi uvicorn[standard]

COPY engram_index/ engram_index/
COPY http_server.py .

ENV ENGRAM_DATA_DIR=/data
ENV ENGRAM_PORT=8100

EXPOSE 8100

CMD ["python", "http_server.py"]
