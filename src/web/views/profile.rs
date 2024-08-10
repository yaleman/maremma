use super::prelude::*;

#[derive(Template)]
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
    let user = claims.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "You must be logged in to view this page".to_string(),
        )
    })?;

    let user: User = user.into();

    Ok(ProfileTemplate {
        title: user.username().clone(),
        username: Some(user.username()),
        profile_user: user,
    })
}
