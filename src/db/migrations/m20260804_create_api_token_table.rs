//! Add persistent metadata used to revoke self-issued API tokens.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260804_create_api_token_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ApiToken::Table)
                    .col(ColumnDef::new(ApiToken::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(ApiToken::Subject).text().not_null())
                    .col(ColumnDef::new(ApiToken::Name).text().not_null())
                    .col(ColumnDef::new(ApiToken::IssuedAt).date_time().not_null())
                    .col(ColumnDef::new(ApiToken::ExpiresAt).date_time().not_null())
                    .col(ColumnDef::new(ApiToken::RevokedAt).date_time())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_api_token_subject")
                    .table(ApiToken::Table)
                    .col(ApiToken::Subject)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ApiToken::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ApiToken {
    Table,
    Id,
    Subject,
    Name,
    IssuedAt,
    ExpiresAt,
    RevokedAt,
}
