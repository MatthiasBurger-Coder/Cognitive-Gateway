FROM python:3.12-slim@sha256:6c4dd321d176d61ea848dc8c73a4f7dbae8f70e0ee48bb411ea2f045b599fa8e
WORKDIR /worker
COPY services/offline-learning/pipeline.py /worker/pipeline.py
USER 10001:10001
ENTRYPOINT ["python", "-I", "/worker/pipeline.py", "execute"]
