use crate::prelude::*;

#[derive(Deserialize, Default, Serialize, Debug, Clone)]
pub struct FakeHost {
    pub services: Vec<String>,
}

#[async_trait]
impl GenericHost for FakeHost {
    /// This is always true because it's the maremma host
    async fn check_up(&self) -> Result<bool, Error> {
        Ok(true)
    }

    fn try_from_config(_config: serde_json::Value) -> Result<Self, Error> {
        Ok(Self {
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::GenericHost;
    use serde_json::json;

    #[tokio::test]
    async fn test_fakehost() {
        let host = FakeHost::try_from_config(json!({})).unwrap();
        assert_eq!(host.check_up().await.unwrap(), true);
    }
}
