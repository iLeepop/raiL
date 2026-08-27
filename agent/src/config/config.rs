use llm::Provider;

#[derive(Debug, Clone)]
pub struct Config {
    pub default_model: Option<String>,
    pub default_provider: Option<Provider>,
    pub default_api_key: Option<String>,
    pub default_base_url: Option<String>,
    pub temperature: Option<f32>,
    pub max_token: Option<u32>,
    pub debug: bool,
    pub log_level: String,
    pub max_history_len: Option<u32>,
}

impl Config {
    pub fn new(
        model: impl Into<String>,
        provider: Provider,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        temperature: f32,
        max_token: u32,
        debug: bool,
        log_level: impl Into<String>,
        max_history_len: u32,
    ) -> Self {
        return Config {
            default_model: Some(model.into()),
            default_provider: Some(provider),
            default_api_key: Some(api_key.into()),
            default_base_url: Some(base_url.into()),
            temperature: Some(temperature),
            max_token: Some(max_token),
            debug,
            log_level: log_level.into(),
            max_history_len: Some(max_history_len),
        };
    }

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }

    pub fn with_default_provider(mut self, provider: Provider) -> Self {
        self.default_provider = Some(provider);
        self
    }

    pub fn with_default_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.default_api_key = Some(api_key.into());
        self
    }

    pub fn with_default_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.default_base_url = Some(base_url.into());
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_token(mut self, max_token: u32) -> Self {
        self.max_token = Some(max_token);
        self
    }

    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    pub fn with_log_level(mut self, log_level: impl Into<String>) -> Self {
        self.log_level = log_level.into();
        self
    }

    pub fn with_max_history_len(mut self, max_history_len: u32) -> Self {
        self.max_history_len = Some(max_history_len);
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        return Config {
            default_model: None,
            default_provider: None,
            default_api_key: None,
            default_base_url: None,
            temperature: None,
            max_token: None,
            debug: false,
            log_level: String::from("info"),
            max_history_len: None,
        };
    }
}
