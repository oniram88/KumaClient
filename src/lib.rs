use anyhow::anyhow;
use log::{debug, error, info};
use rust_socketio::client::Client;
use rust_socketio::{ClientBuilder, Payload, RawClient, TransportType};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: Option<u8>,
    pub name: String,
    #[serde(rename = "type")]
    typology: MonitorType, //type è una chiave non utilizzabile
    pub url: Option<String>,
    pub parent: Option<u64>,
    #[serde(skip_serializing)]
    path_name: Option<String>,
    #[serde(rename = "accepted_statuscodes")]
    accepted_status_codes: Vec<String>,
    expiry_notification: bool,
    method: Option<MonitorMethod>,
    pub interval: u8,
}

impl Monitor {
    pub fn new(name: String, typology: Option<MonitorType>) -> Self {
        Monitor {
            id: None,
            name: name.clone(),
            typology: typology.unwrap_or(MonitorType::Http),
            url: None,
            parent: None,
            path_name: None,
            accepted_status_codes: vec!["200-299".to_string()],
            expiry_notification: true,
            method: Some(MonitorMethod::Head),
            interval: 90
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum MonitorMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

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

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct KumaAuthentication {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

pub struct KumaClient {
    auth: KumaAuthentication,
    entrypoint: String,
    _connected: bool,
    _client: Option<Client>,
    monitor_list: Arc<Mutex<HashMap<String, Monitor>>>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ApiResponse {
    ok: bool,
    msg: Option<String>,
    #[serde(rename = "monitorID")]
    monitor_id: Option<u8>,
}

impl KumaClient {
    pub fn new(entrypoint: String, auth: KumaAuthentication) -> Self {
        KumaClient {
            auth,
            monitor_list: Arc::new(Mutex::new(HashMap::new())),
            _connected: false,
            _client: None,
            entrypoint,
        }
    }

    fn connect(&mut self) -> anyhow::Result<&Self> {
        // SE siamo già connessi non dobbiamo ripetere la connessione
        if self._connected {
            return Ok(self);
        }

        let error_handler = |err, _| error!("Error: {:#?}", err);

        let client_monitor_list = self.monitor_list.clone();
        let client = ClientBuilder::new(format!("{}/", self.entrypoint))
            //.on_any( callback)
            .on("error", error_handler)
            .transport_type(TransportType::Websocket)
            .on("monitorList", move |payload, _| match payload {
                Payload::Binary(_) => {}
                Payload::String(data) => {
                    let tmp_monitor_list: HashMap<String, Monitor> =
                        serde_json::from_str(&data).unwrap();

                    let mut list = client_monitor_list.lock().unwrap();
                    for (_index, monitor) in tmp_monitor_list {
                        list.insert(monitor.uid(), monitor.clone());
                    }
                }
            })
            .reconnect_on_disconnect(true)
            .connect()?;
        let duration = Duration::from_millis(300);
        sleep(duration);

        let ack_callback = |message: Payload, _socket: RawClient| {
            info!("Abbiamo eseguito il login {:#?}", message);
        };

        client.emit_with_ack(
            "login",
            serde_json::to_string(&self.auth).unwrap(),
            Duration::from_secs(2),
            ack_callback,
        )?;
        self._client = Some(client);

        self._connected = true;

        Ok(self)
    }

    pub fn disconnect(&mut self) -> &Self {
        self._client.as_ref().unwrap().disconnect().unwrap();
        self._client = None;

        self._connected = false;

        self
    }

    fn reload_monitor_list(&mut self) -> anyhow::Result<()> {
        self.connect()?;

        self.monitor_list.clone().lock().unwrap().clear();
        self._client
            .as_ref()
            .unwrap()
            .emit("getMonitorList", json!([]))?;

        while self.monitor_list.clone().lock().unwrap().len() == 0 {
            sleep(Duration::from_millis(100));
            //debug!("Attesa load monitors");
        }

        debug!(
            "Monitors caricati: {}",
            self.monitor_list.try_lock().unwrap().len()
        );

        Ok(())
    }

    pub fn add_monitor(&mut self, mut monitor: Monitor) -> anyhow::Result<Monitor> {
        self.connect()?;
        self.reload_monitor_list()?;
        // controllo se è già presente il monitor
        if self
            .monitor_list
            .try_lock()
            .unwrap()
            .contains_key(&monitor.uid())
        {
            return Err(anyhow!("Monitor già presente {}", monitor.uid()));
        }

        // dobbiamo mandare la chiamata di aggiunta monitor e avere la relativa risposta affermativa di aggiunta
        let response: Arc<Mutex<Option<u8>>> = Arc::new(Mutex::new(None));

        let inner_response = response.clone();
        let ack_callback = move |message: Payload, _socket: RawClient| {
            info!("Monitor aggiunto? {:#?}", message);

            match message {
                Payload::Binary(_) => {
                    *inner_response.lock().unwrap() = Some(0);
                }
                Payload::String(data) => {
                    let tmp_result: Vec<ApiResponse> = serde_json::from_str(&data).unwrap();
                    if let Some(response) = tmp_result.first() {
                        if response.ok {
                            *inner_response.lock().unwrap() = Some(response.monitor_id.unwrap_or(0))
                        } else {
                            *inner_response.lock().unwrap() = Some(0)
                        }
                    } else {
                        *inner_response.lock().unwrap() = Some(0)
                    }
                }
            }
        };
        self._client.as_ref().unwrap().emit_with_ack(
            "add",
            json!(monitor),
            Duration::from_secs(4),
            ack_callback,
        )?;

        let mut counter = 0;
        while response.lock().unwrap().is_none() && counter < 20
        //max loop check 20*100ms
        {
            counter = counter + 1;
            sleep(Duration::from_millis(100));
            debug!("Stiamo iterando nell'attesa della callback dell'aggiunta monitor")
        }

        if let Some(id) = response.clone().lock().unwrap().deref() {
            if *id > 0 {
                monitor.id = Some((*id).clone());
                info!("Inserimento Monitor Eseguito");
                return Ok(monitor);
            }
        }
        Err(anyhow!("Errore nella creazione del monitor"))
    }

    pub fn search_monitor(
        &mut self,
        by_name: Option<String>,
        by_parent_id: Option<u64>,
    ) -> HashMap<String, Monitor> {
        if self.reload_monitor_list().is_err() {
            return HashMap::new();
        }
        let mut intermediary = self.monitor_list.lock().unwrap().clone();

        if let Some(name) = by_name {
            intermediary.retain(|_, monitor| monitor.name == name);
        }

        if let Some(parent_id) = by_parent_id {
            intermediary.retain(|_, monitor| monitor.parent.unwrap() == parent_id);
        }

        intermediary
    }

    pub fn find_monitor(
        &mut self,
        by_name: Option<String>,
        by_parent_id: Option<u64>,
    ) -> Option<Monitor> {
        let searched_monitors = self.search_monitor(by_name, by_parent_id);

        if searched_monitors.len() > 1 {
            Some(searched_monitors.values().next().unwrap().clone())
        } else {
            None
        }
    }
    /*
       pub fn add_monitor_tag(&self, monitor: Monitor, tag_id: u8) -> anyhow::Result<()> {
           let response: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

           let inner_response = response.clone();
           let ack_callback = move |message: Payload, _socket: RawClient| {
               info!("Monitor TAG? {:#?}", message);
               //let _ = inner_response.lock().unwrap().insert(true);
           };

           self._client
               .as_ref()
               .unwrap()
               .emit("addMonitorTag", vec![tag_id, monitor.id.unwrap(), 0])
               .expect("Aggiunta Tag Fallito");

           sleep(Duration::from_millis(100));
           /*        while response.lock().unwrap().is_none() {
                       sleep(Duration::from_millis(25));
                       debug!("Stiamo iterando nell'attesa della callback SET TAG")
                   }
           */
           //   if response.lock().unwrap().unwrap() {
           //     info!("Inserimento Monitor Eseguito");
           Ok(())
           //} else {
           //  Err(anyhow!("Errore nella creazione del monitor"))
           //}
       }

    */
}
