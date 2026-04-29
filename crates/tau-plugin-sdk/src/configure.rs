//! [`Configure`] trait for plugins that consume static config from the
//! handshake.
//!
//! Plugin authors who need to read the host's `HandshakeRequest.config`
//! JSON value implement [`Configure`] in addition to their port trait
//! (e.g. [`tau_ports::LlmBackend`]) and call one of the
//! `run_*_with_config` runner flavors. The runner deserializes the
//! handshake `config` field as `T::Config` and constructs the plugin
//! via [`Configure::from_config`] before entering the dispatch loop.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md`
//! §5.4.

use thiserror::Error;

/// Errors raised by [`Configure::from_config`] implementations.
///
/// `#[non_exhaustive]`: additive variants are non-breaking. Plugins
/// should return one of the typed variants below; there is no generic
/// `Internal` escape hatch.
///
/// # Example
///
/// ```ignore
/// use tau_plugin_sdk::ConfigError;
/// let err = ConfigError::MissingField("api_key");
/// assert!(format!("{err}").contains("missing"));
/// ```
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ConfigError {
    /// JSON config payload could not be deserialized as the plugin's
    /// `Configure::Config` type.
    #[error("config decode failed: {0}")]
    Decode(#[from] serde_json::Error),

    /// A required config field was missing.
    #[error("missing required config field: {0}")]
    MissingField(&'static str),

    /// A config field had an unsupported value.
    #[error("invalid value for config field {field}: {detail}")]
    InvalidValue {
        /// The name of the offending field.
        field: &'static str,
        /// Human-readable explanation of why the value was rejected.
        detail: String,
    },
}

/// Trait implemented by plugins that consume the handshake's `config`
/// field. The runner deserializes the JSON config as
/// [`Configure::Config`] and calls [`Configure::from_config`] before
/// entering the dispatch loop.
///
/// Plugins that don't need static config call [`crate::run_llm_backend`]
/// / [`crate::run_tool`] directly with a pre-constructed instance and
/// don't need to implement this trait.
///
/// # Example
///
/// ```ignore
/// use serde::Deserialize;
/// use tau_plugin_sdk::{ConfigError, Configure};
///
/// #[derive(Deserialize)]
/// struct MyConfig {
///     api_key: String,
/// }
///
/// struct MyPlugin {
///     api_key: String,
/// }
///
/// impl Configure for MyPlugin {
///     type Config = MyConfig;
///     fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
///         if config.api_key.is_empty() {
///             return Err(ConfigError::MissingField("api_key"));
///         }
///         Ok(MyPlugin { api_key: config.api_key })
///     }
/// }
/// ```
pub trait Configure: Sized {
    /// The plugin author's config shape. Deserialized from
    /// [`tau_plugin_protocol::HandshakeRequest::config`] before the
    /// runner enters the dispatch loop.
    type Config: serde::de::DeserializeOwned;

    /// Construct the plugin from the deserialized config.
    ///
    /// # Errors
    ///
    /// Return [`ConfigError::MissingField`] for required fields that
    /// were absent or [`ConfigError::InvalidValue`] for fields that
    /// were present but unacceptable.
    fn from_config(config: Self::Config) -> Result<Self, ConfigError>;
}
