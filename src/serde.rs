use croner::Cron;

pub(crate) fn deserialize_croner_cron<'de, D>(deserializer: D) -> Result<Cron, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    Cron::new(&s).parse().map_err(serde::de::Error::custom)
}

pub(crate) fn serialize_croner_cron<S>(cron: &Cron, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&cron.pattern.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use croner::Cron;

    #[test]
    fn test_serde_croner() {
        #[derive(Deserialize, Serialize)]
        struct CronTest {
            #[serde(
                serialize_with = "serialize_croner_cron",
                deserialize_with = "deserialize_croner_cron"
            )]
            cronvalue: Cron,
        }

        let test = serde_json::json! {{"cronvalue": "@hourly"}};

        let res: CronTest = serde_json::from_value(test).unwrap();

        let serialized = serde_json::to_string(&res).unwrap();
        eprintln!("{}", &serialized);

        assert_eq!(r#"{"cronvalue":"0 * * * *"}"#, serialized);

        // assert_eq!(res.cronvalue, Cron::new("@hourly").parse().unwrap());
    }
}
