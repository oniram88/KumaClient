use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub name: String,
    #[serde(rename = "type")]
    typology: MonitorType, //type è una chiave non utilizzabile
    pub url: Option<String>,
    pub parent: Option<u64>,
    #[serde(skip_serializing)]
    path_name: Option<String>,
    #[serde(rename = "accepted_statuscodes")]
    accepted_status_codes: Vec<String>,
}

impl Monitor {
    pub fn new(name: String, typology: Option<MonitorType>) -> Self {
        Monitor {
            name: name.clone(),
            typology: typology.unwrap_or(MonitorType::Http),
            url: None,
            parent: None,
            path_name: None,
            accepted_status_codes: vec!["200-299".to_string()],
        }
    }

    pub fn uid(&self) -> String {
        format!("{}-{}", self.parent.unwrap_or(0), self.name.clone())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MonitorType {
    Http,
    Port,
    Ping,
    Keyword,
    #[serde(rename = "grpc-keyword")]
    GrpcKeyword,
    Dns,
    Docker,
    Push,
    Steam,
    Gamedig,
    Group,
    Mqtt,
    Sqlserver,
    Postgres,
    Mysql,
    Mongodb,
    Radius,
    Redis,
}

mod kuma;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let m = Monitor::new("entrypoint".to_string(), None);
        assert_eq!(m.typology, MonitorType::Http);
        assert!(m.accepted_status_codes.contains(&"200-299".to_string()));
    }
}
