# Automation API

Maremma exposes a bearer-authenticated JSON API for reading monitoring state and
controlling service checks. The generated OpenAPI document is available at
`/api-docs/openapi.json`; Swagger UI is available at `/swagger-ui/`.

## Enable bearer tokens

Set `MAREMMA_JWT_SIGNING_SECRET` before starting Maremma. It must be a secret
between 32 and 64 bytes long and must be kept outside the tracked configuration
file. Store it in your secret manager or service environment; changing it
invalidates every existing bearer token.

## Create and revoke tokens

Sign in through OIDC and open **Profile**. Enter a token name and choose a
lifetime from one to 2,160 hours (30 days is the default). Maremma displays the
token only in the creation response, so copy it immediately. The Profile page
also lists active tokens and lets you revoke one immediately.

Tokens are bound to the OIDC subject that created them. They inherit the same
trusted-user permissions as the existing web interface. They expire after at
most 90 days; revocation is checked on every API request.

## Use the API

Supply the copied value as a bearer token:

```sh
curl --fail-with-body \
  -H "Authorization: Bearer $MAREMMA_API_TOKEN" \
  https://maremma.example.test/api/v1/service-checks
```

The API exposes read-only host, host-group, service, and service-check
resources. It also supports setting a service check to urgent, enabling,
disabling, or deleting it. For the exact JSON schemas and request paths, use
the OpenAPI document or Swagger UI.
