use sea_orm::entity::prelude::*;

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, name = "id")]
    pub id: Uuid,
    pub preferred_username: String,
    pub display_name: String,
    groups: Json,
    claim_json: Json,
}

impl Model {
    /// Returns the list of groups the user is a member of.
    pub fn groups(&self) -> Vec<String> {
        self.groups
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use crate::db::tests::test_setup;

    use super::*;

    #[tokio::test]
    async fn test_user() {
        let (db, _config) = test_setup()
            .await
            .expect("Failed to set up maremma test db");

        let mut user = ActiveModel::new();
        user.id.set_if_not_equals(Uuid::new_v4());
        user.preferred_username
            .set_if_not_equals("Test User".to_string());
        user.display_name.set_if_not_equals("Test User".to_string());
        user.groups.set_if_not_equals(json!(["test"]));
        user.claim_json.set_if_not_equals(json!({}));

        let user = user
            .insert(db.as_ref())
            .await
            .expect("Failed to insert test user!");

        assert_eq!(user.preferred_username, "Test User");
        assert_eq!(user.display_name, "Test User");
        assert_eq!(user.groups(), vec!["test".to_string()]);
    }
}
