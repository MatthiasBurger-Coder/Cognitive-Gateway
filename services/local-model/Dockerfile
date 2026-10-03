FROM python:3.12.11-slim
WORKDIR /service
COPY services/local-model/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY services/local-model/service.py services/local-model/signals.py services/local-model/benchmark.py .
COPY schemas/model-profile.schema.json schemas/model-benchmark-dataset.schema.json /schemas/
COPY models /models
ENV CG_MODEL_REGISTRY=/state/registry.json CG_MODEL_SCHEMA=/schemas/model-profile.schema.json CG_MODEL_SUITE=/models/suites/semantic-proposal-v1.json
EXPOSE 8091
CMD ["python", "service.py", "serve"]
