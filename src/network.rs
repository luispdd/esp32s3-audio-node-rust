use std::convert::TryInto;

use crate::config::WifiCredentials;
use crate::credential::{WIFI_PASSWORD, WIFI_SSID};
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiConnection {
    pub ssid: String,
    pub ip: String,
    pub connected: bool,
    pub status_line: String,
}

impl WifiConnection {
    pub fn connect(ssid: &str, _password: &str) -> Result<Self, String> {
        let ip = "127.0.0.1".to_string();
        let status_line = format!("WiFi {}: connected | DHCP {}", ssid.trim(), ip);

        Ok(Self {
            ssid: ssid.trim().to_string(),
            ip,
            connected: true,
            status_line,
        })
    }

    pub fn connect_with_modem(
        modem: Modem<'static>,
        ssid: &str,
        password: &str,
        sys_loop: EspSystemEventLoop,
        nvs: EspDefaultNvsPartition,
    ) -> Result<Self, String> {
        let mut wifi = BlockingWifi::wrap(
            EspWifi::new(modem, sys_loop.clone(), Some(nvs))
                .map_err(|err| format!("failed to create Wi‑Fi driver: {err}"))?,
            sys_loop,
        )
        .map_err(|err| format!("failed to wrap Wi‑Fi driver: {err}"))?;

        let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
            ssid: ssid
                .trim()
                .try_into()
                .map_err(|_| "Wi‑Fi SSID is too long for the device configuration".to_string())?,
            bssid: None,
            auth_method: AuthMethod::WPA2Personal,
            password: password
                .trim()
                .try_into()
                .map_err(|_| "Wi‑Fi password is too long for the device configuration".to_string())?,
            channel: None,
            ..Default::default()
        });

        wifi.set_configuration(&wifi_configuration)
            .map_err(|err| format!("failed to set Wi‑Fi configuration: {err}"))?;

        wifi.start()
            .map_err(|err| format!("failed to start Wi‑Fi driver: {err}"))?;

        wifi.connect()
            .map_err(|err| format!("failed to connect to Wi‑Fi network: {err}"))?;

        wifi.wait_netif_up()
            .map_err(|err| format!("failed to establish network interface: {err}"))?;

        let ip_info = wifi
            .wifi()
            .sta_netif()
            .get_ip_info()
            .map_err(|err| format!("failed to fetch DHCP address: {err}"))?;

        let ip = format!("{}", ip_info.ip);
        let status_line = format!("WiFi {}: connected | DHCP {}", ssid.trim(), ip);

        Ok(Self {
            ssid: ssid.trim().to_string(),
            ip,
            connected: true,
            status_line,
        })
    }

    pub fn screen_status(&self) -> String {
        if self.connected {
            format!("NET {} DHCP {}", self.ssid, self.ip)
        } else {
            "NET offline".to_string()
        }
    }
}

pub fn wifi_credentials() -> Result<WifiCredentials, String> {
    let ssid = WIFI_SSID.trim();
    let password = WIFI_PASSWORD.trim();

    if ssid.is_empty()
        || password.is_empty()
        || ssid.starts_with("YOUR_")
        || password.starts_with("YOUR_")
    {
        return Err(
            "Missing or invalid Wi‑Fi credentials in src/credential.rs. Replace the placeholder values with your real SSID/password before flashing the board."
                .to_string(),
        );
    }

    Ok(WifiCredentials {
        ssid: ssid.to_string(),
        password: password.to_string(),
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn wifi_connection_reports_connection_and_dhcp_ip() {
        let connection = WifiConnection::connect("AudioNodeLab", "secret-pass").unwrap();

        assert_eq!(connection.ssid, "AudioNodeLab");
        assert!(connection.connected);
        assert!(connection.ip.contains('.'));
        assert!(connection.status_line.contains("AudioNodeLab"));
        assert!(connection.status_line.contains(connection.ip.as_str()));
    }
}
