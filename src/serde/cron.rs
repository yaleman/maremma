use croner::Cron;
use std::str::FromStr;

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Cron, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;

    // ignore for code coverage because for some reason it doesn't pick it up?
    #[cfg(not(tarpaulin_include))]
    Cron::from_str(&s).map_err(serde::de::Error::custom)
}

pub(crate) fn serialize<S>(cron: &Cron, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&cron.pattern.to_string())
}

#[cfg(test)]
mod tests {

    use crate::prelude::*;
    use chrono::Timelike;
    use croner::Cron;
    use std::str::FromStr;

    #[test]
    fn test_serde_croner() {
        #[derive(Deserialize, Serialize)]
        struct CronTest {
            #[serde(with = "super")]
            cronvalue: Cron,
        }

        assert!(serde_json::from_str::<CronTest>("[[[").is_err());

        let test = serde_json::json! {{"cronvalue": "0 * * * *"}};

        let res: CronTest = serde_json::from_value(test).expect("Failed to deserialize");

        let expected_cron = Cron::from_str("0 * * * *")
            .expect("Failed to build cron expression");

        let time = chrono::Local::now()
            .with_minute(59)
            .expect("Failed to parse time");

        assert_eq!(
            res.cronvalue
                .find_next_occurrence(&time, false)
                .expect("Failed to get next occurrence"),
            expected_cron
                .find_next_occurrence(&time, false)
                .expect("Failed to get next occurrence"),
        );

        let serialized = serde_json::to_string(&res).expect("Failed to serialize");
        dbg!(&serialized);

        assert_eq!(r#"{"cronvalue":"0 * * * *"}"#, serialized);

        let failed =
            serde_json::from_value::<CronTest>(serde_json::json! {{"cronvalue": "invalid"}});
        assert!(failed.is_err());
    }
}
