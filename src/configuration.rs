use crate::domain::SubscriberEmail;
use crate::email_client::EmailClient;
use secrecy::{ExposeSecret, Secret};
use serde_aux::field_attributes::deserialize_number_from_string;
use sqlx::postgres::{PgConnectOptions};
use sqlx::ConnectOptions;
use std::convert::{TryFrom, TryInto};
use std::str::FromStr;

#[derive(serde::Deserialize, Clone)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    pub email_client: EmailClientSettings,
    pub redis_uri: Secret<String>,
}

#[derive(serde::Deserialize, Clone)]
pub struct ApplicationSettings {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub base_url: String,
    pub hmac_secret: Secret<String>,
}

#[derive(serde::Deserialize, Clone)]
pub struct DatabaseSettings {
    pub uri: Secret<String>,
    pub database_name: Option<String>,
}

impl DatabaseSettings {
    pub fn without_db(&self) -> PgConnectOptions {
        let options = PgConnectOptions::from_str(self.uri.expose_secret()).expect("Could not parse database URI");
        if options.get_database().is_some() {
            panic!("A database was provided in the URI but this should not happen in this environment, move the name to the database_name field instead.");
        }
        options
    }

    pub fn with_db(&self) -> PgConnectOptions {
        let mut options = PgConnectOptions::from_str(self.uri.expose_secret()).expect("Could not parse database URI");
        if options.get_database().is_none() {
            if let Some(database_name) = &self.database_name {
                options = options.database(&database_name)
            }
            else {
                panic!("No database was specified in the URI and none was specified in the database_name setting either.");
            }
        };
        options.log_statements(tracing::log::LevelFilter::Trace);
        options
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct EmailClientSettings {
    pub base_url: String,
    pub sender_email: String,
    pub authorization_token: Secret<String>,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub timeout_milliseconds: u64,
}

impl EmailClientSettings {
    pub fn client(self) -> EmailClient {
        let sender_email = self.sender().expect("Invalid sender email address.");
        let timeout = self.timeout();
        EmailClient::new(
            self.base_url,
            sender_email,
            self.authorization_token,
            timeout,
        )
    }

    pub fn sender(&self) -> Result<SubscriberEmail, String> {
        SubscriberEmail::parse(self.sender_email.clone())
    }

    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_milliseconds)
    }
}

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let base_path = std::env::current_dir().expect("Failed to determine the current directory");
    let configuration_directory = base_path.join("configuration");

    // Detect the running environment.
    // Default to `local` if unspecified.
    let environment: Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "local".into())
        .try_into()
        .expect("Failed to parse APP_ENVIRONMENT.");
    let environment_filename = format!("{}.yaml", environment.as_str());
    let mut config_builder = config::Config::builder()
        .add_source(config::File::from(
            configuration_directory.join("base.yaml"),
        ))
        .add_source(config::File::from(
            configuration_directory.join(&environment_filename),
        ))
        // Add in settings from environment variables (with a prefix of APP and '__' as separator)
        // E.g. `APP_APPLICATION__PORT=5001 would set `Settings.application.port`
        .add_source(
            config::Environment::with_prefix("APP")
                .prefix_separator("_")
                .separator("__"),
        );

    // Allow PORT to override APPLICATION_PORT
    if let Ok(port) = std::env::var("PORT") {
        config_builder = config_builder.set_override("application.port", port).expect("Could not set port from PORT env variable");
    }

    // Allow FC_URL to override APPLICATION__BASE_URL
    if let Ok(base_url) = std::env::var("FC_URL") {
        config_builder = config_builder.set_override("application.base_url", base_url).expect("Could not set base_url from FC_URL env variable");
    }

    let settings = config_builder.build()?;

    settings.try_deserialize::<Settings>()
}

/// The possible runtime environment for our application.
pub enum Environment {
    Local,
    Production,
}

impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Local => "local",
            Environment::Production => "production",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            other => Err(format!(
                "{} is not a supported environment. Use either `local` or `production`.",
                other
            )),
        }
    }
}
