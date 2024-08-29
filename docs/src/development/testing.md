# Testing things

There's a bunch of "live" tests that you need to set environment variables for, because I'm not telling everyone which hosts and checks I run locally :)

Here's the (as of 2024-08-24) envs:

- MAREMMA_TEST_SSH_HOST
- MAREMMA_TEST_KUBE_HOST
- MAREMMA_TEST_SSH_HOST
- MAREMMA_TEST_SSH_USERNAME
- MAREMMA_TEST_SSH_KEY

You need docker-or-some-docker-compatible thing running, it'll run Nginx using [testcontainers](https://crates.io/crates/testcontainers) to test TLS checks.

BadSSL are a great source of "broken" certificates, so it'll try and hit those endpoints. They're pretty strict on things accessing their endpoints programatically, so don't be surprised when you get blocked, that's why this feature is not enabled by default.
