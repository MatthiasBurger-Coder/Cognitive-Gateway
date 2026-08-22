# Inbound Adapters

Inbound adapters are driving adapters. They translate requests from external clients such as CLI, API, IDE and CI integrations into `gateway-application` inbound port calls.

No transport framework or client-specific behavior belongs in `gateway-domain`. Concrete inbound adapters are introduced by later implementation slices.
