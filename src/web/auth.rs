//! JWT bearer-token signing and validation.

use std::str::FromStr;

use compact_jwt::{JwsHs256Signer, JwsSigner, JwsVerifier, Jwt, JwtUnverified};
use uuid::Uuid;

use crate::prelude::*;

pub(crate) const JWT_SIGNING_SECRET_ENV: &str = "MAREMMA_JWT_SIGNING_SECRET";
const TOKEN_ISSUER: &str = "maremma";

#[derive(Clone)]
pub(crate) struct JwtAuth {
    signer: JwsHs256Signer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedToken {
    pub(crate) subject: String,
    pub(crate) token_id: Uuid,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum JwtAuthError {
    MissingSigningSecret,
    InvalidSigningSecret,
    InvalidToken,
    ExpiredToken,
}

impl JwtAuth {
    pub(crate) fn from_environment() -> Result<Self, JwtAuthError> {
        let secret = std::env::var(JWT_SIGNING_SECRET_ENV)
            .map_err(|_| JwtAuthError::MissingSigningSecret)?;
        Self::from_secret(&secret)
    }

    pub(crate) fn from_secret(secret: &str) -> Result<Self, JwtAuthError> {
        JwsHs256Signer::try_from(secret.as_bytes())
            .map(|signer| Self { signer })
            .map_err(|_| JwtAuthError::InvalidSigningSecret)
    }

    pub(crate) fn issue(
        &self,
        subject: String,
        token_id: Uuid,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<String, JwtAuthError> {
        let token = Jwt::<()> {
            iss: Some(TOKEN_ISSUER.to_string()),
            sub: Some(subject),
            exp: Some(expires_at.timestamp()),
            iat: Some(issued_at.timestamp()),
            jti: Some(token_id.to_string()),
            extensions: (),
            ..Default::default()
        };

        self.signer
            .sign(&token)
            .map(|signed| signed.to_string())
            .map_err(|_| JwtAuthError::InvalidToken)
    }

    pub(crate) fn validate(
        &self,
        encoded_token: &str,
        now: DateTime<Utc>,
    ) -> Result<AuthenticatedToken, JwtAuthError> {
        let unverified =
            JwtUnverified::<()>::from_str(encoded_token).map_err(|_| JwtAuthError::InvalidToken)?;
        let token = self
            .signer
            .verify(&unverified)
            .map_err(|_| JwtAuthError::InvalidToken)?;

        if token.iss.as_deref() != Some(TOKEN_ISSUER) {
            return Err(JwtAuthError::InvalidToken);
        }

        let expiration = token.exp.ok_or(JwtAuthError::InvalidToken)?;
        if expiration <= now.timestamp() {
            return Err(JwtAuthError::ExpiredToken);
        }

        let issued_at = token.iat.ok_or(JwtAuthError::InvalidToken)?;
        if issued_at > now.timestamp() {
            return Err(JwtAuthError::InvalidToken);
        }

        let subject = token
            .sub
            .filter(|subject| !subject.trim().is_empty())
            .ok_or(JwtAuthError::InvalidToken)?;
        let token_id = token
            .jti
            .as_deref()
            .ok_or(JwtAuthError::InvalidToken)
            .and_then(|jti| Uuid::parse_str(jti).map_err(|_| JwtAuthError::InvalidToken))?;

        Ok(AuthenticatedToken { subject, token_id })
    }
}

#[cfg(test)]
pub(crate) fn test_auth() -> Result<JwtAuth, JwtAuthError> {
    JwtAuth::from_secret("0123456789abcdef0123456789abcdef")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_token_validates() {
        let auth = test_auth().expect("Failed to create test auth");
        let now = Utc::now();
        let token_id = Uuid::new_v4();
        let token = auth
            .issue(
                "test-subject".to_string(),
                token_id,
                now,
                now + Duration::hours(1),
            )
            .expect("Failed to issue token");

        assert_eq!(
            auth.validate(&token, now)
                .expect("Failed to validate token"),
            AuthenticatedToken {
                subject: "test-subject".to_string(),
                token_id,
            }
        );
    }

    #[test]
    fn invalid_signing_secret_is_rejected() {
        assert!(matches!(
            JwtAuth::from_secret("too short"),
            Err(JwtAuthError::InvalidSigningSecret)
        ));
    }

    #[test]
    fn malformed_wrong_key_expired_and_invalid_claim_tokens_are_rejected() {
        let auth = test_auth().expect("Failed to create test auth");
        let wrong_auth = JwtAuth::from_secret("abcdef0123456789abcdef0123456789")
            .expect("Failed to create second auth");
        let now = Utc::now();
        let token = auth
            .issue(
                "test-subject".to_string(),
                Uuid::new_v4(),
                now - Duration::hours(2),
                now - Duration::hours(1),
            )
            .expect("Failed to issue token");

        assert_eq!(
            auth.validate("not-a-jwt", now),
            Err(JwtAuthError::InvalidToken)
        );
        assert_eq!(
            wrong_auth.validate(&token, now),
            Err(JwtAuthError::InvalidToken)
        );
        assert_eq!(auth.validate(&token, now), Err(JwtAuthError::ExpiredToken));

        let invalid_issuer = auth
            .signer
            .sign(&Jwt::<()> {
                iss: Some("not-maremma".to_string()),
                sub: Some("test-subject".to_string()),
                exp: Some((now + Duration::hours(1)).timestamp()),
                iat: Some(now.timestamp()),
                jti: Some(Uuid::new_v4().to_string()),
                extensions: (),
                ..Default::default()
            })
            .expect("Failed to issue invalid-claim test token")
            .to_string();
        assert_eq!(
            auth.validate(&invalid_issuer, now),
            Err(JwtAuthError::InvalidToken)
        );
    }
}
