use crate::prelude::*;

#[allow(dead_code)]
#[derive(Deserialize, Default, Serialize, Debug)]
pub struct FakeHost {
    pub services: Vec<String>,
}

#[async_trait]
impl GenericHost for FakeHost {
    fn id(&self) -> String {
        "fakehost".to_string()
    }
    fn name(&self) -> String {
        "FakeHost".to_string()
    }
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
