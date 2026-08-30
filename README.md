# Maremma

> [!WARNING]
> This is an inherently unstable platform, it has weird issues with concurrency I haven't worked out
> and ... you really shouldn't use it!

Monitoring system for guarding your herd 🐐🐐 🐕

Inspired by every other active monitoring platform.

[![Coverage Status](https://coveralls.io/repos/github/yaleman/maremma/badge.svg?branch=main)](https://coveralls.io/github/yaleman/maremma?branch=main)

## OpenTelemetry tracing

Maremma exports traces over OTLP/HTTP protobuf when either `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` or
`OTEL_EXPORTER_OTLP_ENDPOINT` is set. Trace export is disabled when neither variable is configured,
while stdout logging remains enabled.

Each service check produces a `check` trace containing the check, service, and target identifiers,
the final status and duration, and an event with the check result. Failed checks set the
OpenTelemetry span status to `ERROR` so they can be filtered directly.

The exporter honours the standard trace-specific or general OTLP endpoint, timeout, and header
environment variables. For OTLP/HTTP, a trace-specific endpoint is used exactly as configured and
must include the trace ingestion path; a general endpoint has `/v1/traces` appended automatically.

## Additional SNMP MIB directories

Maremma can expose administrator-managed MIB directories to CLI checks through Net-SNMP's `MIBDIRS`
environment variable. Mount and maintain the directories separately, then list them in the global
configuration:

```json
{
  "mib_include_paths": [
    "/config/mibs",
    "/config/vendor-mibs/tplink"
  ]
}
```

Every configured path must exist and be a directory when the configuration is loaded. Maremma adds
the paths to Net-SNMP's normal search directories; it does not download, extract, or update MIB
bundles.

## Docker-container specific notes

- the monitoring-plugins.org plugins end up in `/usr/local/bin/` and so does `check_splunk` - so
  base your commands off that.
