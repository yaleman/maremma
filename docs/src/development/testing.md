# Testing things

There's a bunch of "live" tests that you need to set environment variables for, because I'm not telling everyone which hosts and checks I run locally :)

Here's the (as of 2024-08-31) envs:

- MAREMMA_FRONTEND_URL
- MAREMMA_OIDC_CLIENT_ID
- MAREMMA_OIDC_ISSUER
- MAREMMA_TEST_KUBE_HOST
- MAREMMA_TEST_SSH_HOST
- MAREMMA_TEST_SSH_KEY
- MAREMMA_TEST_SSH_USERNAME
- MAREMMA_TEST_PUSHOVER_TOKEN
- MAREMMA_TEST_PUSHOVER_USER

You need docker-or-some-docker-compatible thing running, it'll run Nginx using [testcontainers](https://crates.io/crates/testcontainers) to test TLS checks.
