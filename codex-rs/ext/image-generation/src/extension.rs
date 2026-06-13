use std::sync::Arc;

use codex_core::config::Config;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_login::AuthManager;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::backend::CodexImagesBackend;
use crate::tool::ImageGenerationTool;

const T3_PROVIDER_NAME: &str = "MyService";
const T3_IDE_JWT_ENV_KEY: &str = "MYIDE_IDE_JWT";

#[derive(Clone)]
struct ImageGenerationExtension {
    auth_manager: Arc<AuthManager>,
}

#[derive(Clone)]
struct ImageGenerationExtensionConfig {
    available: bool,
    provider: ModelProviderInfo,
    codex_home: AbsolutePathBuf,
}

impl From<&Config> for ImageGenerationExtensionConfig {
    /// Resolves whether standalone image generation should be available for a thread.
    fn from(config: &Config) -> Self {
        Self {
            available: standalone_image_generation_available_for_provider(&config.model_provider),
            provider: config.model_provider.clone(),
            codex_home: config.codex_home.clone(),
        }
    }
}

impl ThreadLifecycleContributor<Config> for ImageGenerationExtension {
    /// Seeds image-generation availability when a thread begins.
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(ImageGenerationExtensionConfig::from(input.config));
        })
    }
}

impl ConfigContributor<Config> for ImageGenerationExtension {
    /// Refreshes image-generation availability after thread configuration changes.
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(ImageGenerationExtensionConfig::from(new_config));
    }
}

impl ToolContributor for ImageGenerationExtension {
    /// Creates the image-generation tool exposed by this installed extension.
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        let Some(config) = thread_store.get::<ImageGenerationExtensionConfig>() else {
            return Vec::new();
        };
        if !config.available || !standalone_image_generation_auth_available(&config.provider) {
            return Vec::new();
        }

        vec![Arc::new(ImageGenerationTool::new(
            CodexImagesBackend::new(create_model_provider(
                config.provider.clone(),
                Some(self.auth_manager.clone()),
            )),
            config.codex_home.clone(),
            thread_store.level_id().to_string(),
        ))]
    }
}

fn standalone_image_generation_available_for_provider(provider: &ModelProviderInfo) -> bool {
    provider.name == T3_PROVIDER_NAME
        && provider.env_key.as_deref() == Some(T3_IDE_JWT_ENV_KEY)
        && provider.base_url.is_some()
        && !provider.requires_openai_auth
}

fn standalone_image_generation_auth_available(provider: &ModelProviderInfo) -> bool {
    let Some(env_key) = provider.env_key.as_deref() else {
        return false;
    };

    std::env::var(env_key)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// Installs the standalone image-generation extension contributors.
pub fn install(registry: &mut ExtensionRegistryBuilder<Config>, auth_manager: Arc<AuthManager>) {
    let extension = Arc::new(ImageGenerationExtension { auth_manager });
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.tool_contributor(extension);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t3_provider() -> ModelProviderInfo {
        ModelProviderInfo {
            name: T3_PROVIDER_NAME.to_string(),
            base_url: Some("https://gateway.example.com/v1".to_string()),
            env_key: Some(T3_IDE_JWT_ENV_KEY.to_string()),
            requires_openai_auth: false,
            ..Default::default()
        }
    }

    #[test]
    fn t3_provider_can_enable_standalone_image_generation() {
        assert!(standalone_image_generation_available_for_provider(
            &t3_provider()
        ));
    }

    #[test]
    fn provider_must_match_t3_gateway_auth_shape() {
        let mut provider = t3_provider();
        provider.name = "OpenAI".to_string();
        assert!(!standalone_image_generation_available_for_provider(
            &provider
        ));

        let mut provider = t3_provider();
        provider.requires_openai_auth = true;
        assert!(!standalone_image_generation_available_for_provider(
            &provider
        ));

        let mut provider = t3_provider();
        provider.env_key = Some("OPENAI_API_KEY".to_string());
        assert!(!standalone_image_generation_available_for_provider(
            &provider
        ));
    }
}
