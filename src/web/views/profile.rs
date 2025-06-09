use super::prelude::*;

#[derive(Template, Debug, WebTemplate)]
#[template(path = "profile.html")]
pub(crate) struct ProfileTemplate {
    title: String,
    username: Option<String>, // for the header
    profile_user: User,
}

pub(crate) async fn profile(
    State(_state): State<WebState>,
    claims: Option<OidcClaims<EmptyAdditionalClaims>>,
) -> Result<ProfileTemplate, (StatusCode, String)> {
    let user = check_login(claims)?;

    Ok(ProfileTemplate {
        title: user.username(),
        username: Some(user.username()),
        profile_user: user,
    })
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_view_profile() {
        use super::*;
        let state = WebState::test().await;

        let res = super::profile(
            State(state.clone()),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;
        dbg!(&res);
        let res_body = res.expect("Failed to get response").to_string();
        assert!(res_body.contains("testuser@example.com"));
        let res = super::profile(
            State(state.clone()),
            Some(crate::web::views::tools::test_user_claims()),
        )
        .await;
        dbg!(&res);
        assert_eq!(
            res.expect("Failed to get response")
                .into_response()
                .status(),
            StatusCode::OK
        )
    }

    #[tokio::test]
    async fn test_view_profile_noauth() {
        use super::*;
        let state = WebState::test().await;

        let res = super::profile(State(state.clone()), None).await;

        dbg!(&res);
        assert!(res.is_err());
        assert_eq!(res.into_response().status(), StatusCode::UNAUTHORIZED)
    }
}
