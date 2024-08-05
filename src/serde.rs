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
    use chrono::Timelike;
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

        let test = serde_json::json! {{"cronvalue": "0 * * * *"}};

        let res: CronTest = serde_json::from_value(test).unwrap();

        let expected_cron = Cron::new("0 * * * *").parse().unwrap();

        let time = chrono::Local::now().with_minute(59).unwrap();

        assert_eq!(
            res.cronvalue.find_next_occurrence(&time, false).unwrap(),
            expected_cron.find_next_occurrence(&time, false).unwrap(),
        );

        let serialized = serde_json::to_string(&res).unwrap();
        eprintln!("{}", &serialized);

        assert_eq!(r#"{"cronvalue":"0 * * * *"}"#, serialized);

        let failed =
            serde_json::from_value::<CronTest>(serde_json::json! {{"cronvalue": "invalid"}});
        assert!(failed.is_err());
    }
}
