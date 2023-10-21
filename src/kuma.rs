use crate::Monitor;
use anyhow::anyhow;
use log::{debug, error, info};
use rust_socketio::client::Client;
use rust_socketio::{ClientBuilder, Payload, RawClient, TransportType};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

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

impl KumaClient {
    pub(crate) fn new(entrypoint: String, auth: KumaAuthentication) -> Self {
        KumaClient {
            auth,
            monitor_list: Arc::new(Mutex::new(HashMap::new())),
            _connected: false,
            _client: None,
            entrypoint,
        }
    }

    fn connect(&mut self) -> &Self {
        // SE siamo già connessi non dobbiamo ripetere la connessione
        if self._connected {
            return self;
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
            .connect()
            .expect("Connection problems");

        let duration = Duration::from_millis(300);
        sleep(duration);

        let ack_callback = |message: Payload, _socket: RawClient| {
            info!("Abbiamo eseguito il login {:#?}", message);
        };

        client
            .emit_with_ack(
                "login",
                serde_json::to_string(&self.auth).unwrap(),
                Duration::from_secs(2),
                ack_callback,
            )
            .expect("No authentication");

        self._client = Some(client);

        self._connected = true;

        self
    }

    pub(crate) fn disconnect(&mut self) -> &Self {
        self._client.as_ref().unwrap().disconnect().unwrap();
        self._client = None;

        self._connected = false;

        self
    }

    pub(crate) fn add_monitor(&mut self, monitor: Monitor) -> anyhow::Result<()> {
        self.connect();
        self._client
            .as_ref()
            .unwrap()
            .emit("getMonitorList", json!([]))
            .expect("Monitor Loading Error");

        while self.monitor_list.clone().lock().unwrap().len() == 0 {
            sleep(Duration::from_millis(50));
            debug!("Attesa load monitors");
        }

        debug!(
            "Monitors caricati: {}",
            self.monitor_list.try_lock().unwrap().len()
        );

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
        let response: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

        let inner_response = response.clone();
        let ack_callback = move |message: Payload, _socket: RawClient| {
            info!("Monitor aggiunto? {:#?}", message);
            let _ = inner_response.lock().unwrap().insert(true);
        };

        self._client
            .as_ref()
            .unwrap()
            .emit_with_ack("add", json!(monitor), Duration::from_secs(2), ack_callback)
            .expect("CREAZIONE FALLITa");

        while response.lock().unwrap().is_none() {
            sleep(Duration::from_millis(25));
            debug!("Stiamo iterando nell'attesa della callback dell'aggiunta monitor")
        }

        if response.lock().unwrap().unwrap() {
            info!("Inserimento Monitor Eseguito");
            Ok(())
        } else {
            Err(anyhow!("Errore nella creazione del monitor"))
        }
    }
}
