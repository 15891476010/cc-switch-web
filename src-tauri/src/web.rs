use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{io, net::SocketAddr, str::FromStr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, services::ServeDir};

use crate::{
    app_config::AppType,
    config,
    database::Database,
    provider::Provider,
    proxy::providers::{
        codex_oauth_auth::{CodexOAuthError, CodexOAuthManager},
        copilot_auth::{CopilotAuthError, CopilotAuthManager, GitHubAccount, GitHubDeviceCodeResponse},
    },
    services::{ProviderService, ProviderSortUpdate},
    store::AppState,
};

const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
const AUTH_PROVIDER_CODEX_OAUTH: &str = "codex_oauth";

#[derive(Clone)]
struct WebState {
    app: Arc<AppState>,
    copilot: Arc<RwLock<CopilotAuthManager>>,
    codex: Arc<RwLock<CodexOAuthManager>>,
}

#[derive(Debug, Clone, Serialize)]
struct ManagedAuthAccount {
    id: String,
    provider: String,
    login: String,
    avatar_url: Option<String>,
    authenticated_at: i64,
    is_default: bool,
    github_domain: String,
}

#[derive(Debug, Clone, Serialize)]
struct ManagedAuthStatus {
    provider: String,
    authenticated: bool,
    default_account_id: Option<String>,
    migration_error: Option<String>,
    accounts: Vec<ManagedAuthAccount>,
}

#[derive(Debug, Clone, Serialize)]
struct ManagedAuthDeviceCodeResponse {
    provider: String,
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key)
}

fn string_arg(args: &Value, key: &str) -> Result<String, String> {
    arg(args, key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("Missing string argument: {key}"))
}

fn optional_string_arg(args: &Value, key: &str) -> Option<String> {
    arg(args, key).and_then(Value::as_str).map(str::to_string)
}

fn bool_arg(args: &Value, key: &str) -> Result<bool, String> {
    arg(args, key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("Missing boolean argument: {key}"))
}

fn optional_i64_arg(args: &Value, key: &str) -> Option<i64> {
    arg(args, key).and_then(Value::as_i64)
}

fn u32_arg(args: &Value, key: &str, default: u32) -> Result<u32, String> {
    match arg(args, key).and_then(Value::as_u64) {
        Some(value) => u32::try_from(value)
            .map_err(|_| format!("Argument out of range for u32: {key}")),
        None => Ok(default),
    }
}

fn parse_app(args: &Value) -> Result<AppType, String> {
    AppType::from_str(&string_arg(args, "app")?).map_err(|e| e.to_string())
}

fn parse_provider(args: &Value) -> Result<Provider, String> {
    serde_json::from_value(
        arg(args, "provider")
            .cloned()
            .ok_or_else(|| "Missing provider argument".to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn map_account(
    provider: &str,
    account: GitHubAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
        github_domain: account.github_domain,
    }
}

fn map_device_code_response(
    provider: &str,
    response: GitHubDeviceCodeResponse,
) -> ManagedAuthDeviceCodeResponse {
    ManagedAuthDeviceCodeResponse {
        provider: provider.to_string(),
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_in: response.expires_in,
        interval: response.interval,
    }
}

fn ok<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

async fn handle_rpc_command(state: WebState, command: &str, args: Value) -> Result<Value, String> {
    match command {
        "get_providers" => {
            let app = parse_app(&args)?;
            ok(ProviderService::list(&state.app, app).map_err(|e| e.to_string())?)
        }
        "get_current_provider" => {
            let app = parse_app(&args)?;
            ok(ProviderService::current(&state.app, app).map_err(|e| e.to_string())?)
        }
        "add_provider" => {
            let app = parse_app(&args)?;
            let provider = parse_provider(&args)?;
            let add_to_live = arg(&args, "addToLive").and_then(Value::as_bool).unwrap_or(true);
            ok(ProviderService::add(&state.app, app, provider, add_to_live)
                .map_err(|e| e.to_string())?)
        }
        "update_provider" => {
            let app = parse_app(&args)?;
            let provider = parse_provider(&args)?;
            let original_id = optional_string_arg(&args, "originalId");
            ok(ProviderService::update(&state.app, app, original_id.as_deref(), provider)
                .map_err(|e| e.to_string())?)
        }
        "delete_provider" => {
            let app = parse_app(&args)?;
            let id = string_arg(&args, "id")?;
            ProviderService::delete(&state.app, app, &id).map_err(|e| e.to_string())?;
            ok(true)
        }
        "remove_provider_from_live_config" => {
            let app = parse_app(&args)?;
            let id = string_arg(&args, "id")?;
            ProviderService::remove_from_live_config(&state.app, app, &id)
                .map_err(|e| e.to_string())?;
            ok(true)
        }
        "switch_provider" => {
            let app = parse_app(&args)?;
            let id = string_arg(&args, "id")?;
            ok(ProviderService::switch(&state.app, app, &id).map_err(|e| e.to_string())?)
        }
        "import_default_config" => {
            let app = parse_app(&args)?;
            ok(ProviderService::import_default_config(&state.app, app)
                .map_err(|e| e.to_string())?)
        }
        "update_providers_sort_order" => {
            let app = parse_app(&args)?;
            let updates: Vec<ProviderSortUpdate> = serde_json::from_value(
                arg(&args, "updates")
                    .cloned()
                    .ok_or_else(|| "Missing updates argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            ok(ProviderService::update_sort_order(&state.app, app, updates)
                .map_err(|e| e.to_string())?)
        }
        "update_tray_menu" => ok(true),
        "get_universal_providers" => {
            ok(ProviderService::list_universal(&state.app).map_err(|e| e.to_string())?)
        }
        "get_universal_provider" => {
            let id = string_arg(&args, "id")?;
            ok(ProviderService::get_universal(&state.app, &id).map_err(|e| e.to_string())?)
        }
        "upsert_universal_provider" => {
            let provider = serde_json::from_value(
                arg(&args, "provider")
                    .cloned()
                    .ok_or_else(|| "Missing provider argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            ok(ProviderService::upsert_universal(&state.app, provider)
                .map_err(|e| e.to_string())?)
        }
        "delete_universal_provider" => {
            let id = string_arg(&args, "id")?;
            ok(ProviderService::delete_universal(&state.app, &id).map_err(|e| e.to_string())?)
        }
        "sync_universal_provider" => {
            let id = string_arg(&args, "id")?;
            ok(ProviderService::sync_universal_to_apps(&state.app, &id)
                .map_err(|e| e.to_string())?)
        }
        "get_settings" => ok(crate::settings::get_settings_for_frontend()),
        "save_settings" => {
            let incoming = serde_json::from_value(
                arg(&args, "settings")
                    .cloned()
                    .ok_or_else(|| "Missing settings argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            crate::settings::update_settings(incoming).map_err(|e| e.to_string())?;
            ok(true)
        }
        "get_config_dir" => {
            let app = parse_app(&args)?;
            let dir = match app {
                AppType::Claude => config::get_claude_config_dir(),
                AppType::Codex => crate::codex_config::get_codex_config_dir(),
                AppType::Gemini => crate::gemini_config::get_gemini_dir(),
                AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
                AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
                AppType::Hermes => crate::hermes_config::get_hermes_dir(),
            };
            ok(dir.to_string_lossy().to_string())
        }
        "get_app_config_path" => ok(config::get_app_config_path().to_string_lossy().to_string()),
        "get_app_config_dir_override" => ok(Value::Null),
        "set_app_config_dir_override" => ok(true),
        "open_config_folder" => ok(true),
        "pick_directory" => {
            let default = optional_string_arg(&args, "defaultPath");
            ok(default.map_or(Value::Null, Value::String))
        }
        "open_app_config_folder" => ok(true),
        "is_portable_mode" => ok(false),
        "get_init_error" => ok(Value::Null),
        "get_migration_result" => ok(false),
        "get_skills_migration_result" => ok(Value::Null),
        "open_external" => ok(true),
        "copy_text_to_clipboard" => ok(true),
        "check_for_updates" => ok(true),
        "get_tool_versions" => ok(Vec::<Value>::new()),
        "check_env_conflicts" => ok(Vec::<Value>::new()),
        "get_common_config_snippet" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_config_snippet(&app_type)
                .map_err(|e| e.to_string())?)
        }
        "set_common_config_snippet" => {
            let app_type = string_arg(&args, "appType")?;
            let snippet = string_arg(&args, "snippet")?;
            let is_cleared = snippet.trim().is_empty();
            let stored = if is_cleared { None } else { Some(snippet) };
            state
                .app
                .db
                .set_config_snippet(&app_type, stored)
                .map_err(|e| e.to_string())?;
            state
                .app
                .db
                .set_config_snippet_cleared(&app_type, is_cleared)
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "extract_common_config_snippet" => {
            let app_type = AppType::from_str(&string_arg(&args, "appType")?)
                .map_err(|e| e.to_string())?;
            if let Some(settings_config) = optional_string_arg(&args, "settingsConfig") {
                let settings: Value =
                    serde_json::from_str(&settings_config).map_err(|e| e.to_string())?;
                ok(ProviderService::extract_common_config_snippet_from_settings(
                    app_type,
                    &settings,
                )
                .map_err(|e| e.to_string())?)
            } else {
                ok(ProviderService::extract_common_config_snippet(&state.app, app_type)
                    .map_err(|e| e.to_string())?)
            }
        }
        "read_live_provider_settings" => {
            let app = parse_app(&args)?;
            ok(ProviderService::read_live_settings(app).map_err(|e| e.to_string())?)
        }
        "fetch_models_for_config" => {
            let base_url = string_arg(&args, "baseUrl")?;
            let api_key = string_arg(&args, "apiKey")?;
            let is_full_url = arg(&args, "isFullUrl")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            ok(crate::services::model_fetch::fetch_models(
                &base_url,
                &api_key,
                is_full_url,
            )
            .await?)
        }
        "start_proxy_server" => ok(state.app.proxy_service.start().await?),
        "stop_proxy_with_restore" => {
            state.app.proxy_service.stop_with_restore().await?;
            ok(Value::Null)
        }
        "get_proxy_status" => ok(state.app.proxy_service.get_status().await?),
        "is_proxy_running" => ok(state.app.proxy_service.is_running().await),
        "is_live_takeover_active" => ok(state.app.proxy_service.is_takeover_active().await?),
        "get_proxy_takeover_status" => ok(state.app.proxy_service.get_takeover_status().await?),
        "set_proxy_takeover_for_app" => {
            let app_type = string_arg(&args, "appType")?;
            let enabled = bool_arg(&args, "enabled")?;
            state
                .app
                .proxy_service
                .set_takeover_for_app(&app_type, enabled)
                .await?;
            ok(Value::Null)
        }
        "switch_proxy_provider" => {
            let app_type = string_arg(&args, "appType")?;
            let provider_id = string_arg(&args, "providerId")?;
            state
                .app
                .proxy_service
                .switch_proxy_target(&app_type, &provider_id)
                .await?;
            ok(Value::Null)
        }
        "get_proxy_config" => ok(state.app.proxy_service.get_config().await?),
        "update_proxy_config" => {
            let config = serde_json::from_value(
                arg(&args, "config")
                    .cloned()
                    .ok_or_else(|| "Missing config argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            state.app.proxy_service.update_config(&config).await?;
            ok(Value::Null)
        }
        "get_global_proxy_config" => {
            ok(state.app.db.get_global_proxy_config().await.map_err(|e| e.to_string())?)
        }
        "update_global_proxy_config" => {
            let config = serde_json::from_value(
                arg(&args, "config")
                    .cloned()
                    .ok_or_else(|| "Missing config argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            state
                .app
                .db
                .update_global_proxy_config(config)
                .await
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "get_proxy_config_for_app" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_proxy_config_for_app(&app_type)
                .await
                .map_err(|e| e.to_string())?)
        }
        "update_proxy_config_for_app" => {
            let config = serde_json::from_value(
                arg(&args, "config")
                    .cloned()
                    .ok_or_else(|| "Missing config argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            state
                .app
                .db
                .update_proxy_config_for_app(config)
                .await
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "get_default_cost_multiplier" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_default_cost_multiplier(&app_type)
                .await
                .map_err(|e| e.to_string())?)
        }
        "set_default_cost_multiplier" => {
            let app_type = string_arg(&args, "appType")?;
            let value = string_arg(&args, "value")?;
            state
                .app
                .db
                .set_default_cost_multiplier(&app_type, &value)
                .await
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "get_pricing_model_source" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_pricing_model_source(&app_type)
                .await
                .map_err(|e| e.to_string())?)
        }
        "set_pricing_model_source" => {
            let app_type = string_arg(&args, "appType")?;
            let value = string_arg(&args, "value")?;
            state
                .app
                .db
                .set_pricing_model_source(&app_type, &value)
                .await
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "get_provider_health" => {
            let provider_id = string_arg(&args, "providerId")?;
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_provider_health(&provider_id, &app_type)
                .await
                .map_err(|e| e.to_string())?)
        }
        "get_circuit_breaker_stats" => ok(Value::Null),
        "reset_circuit_breaker" => {
            let provider_id = string_arg(&args, "providerId")?;
            let app_type = string_arg(&args, "appType")?;
            state
                .app
                .db
                .update_provider_health(&provider_id, &app_type, true, None)
                .await
                .map_err(|e| e.to_string())?;
            state
                .app
                .proxy_service
                .reset_provider_circuit_breaker(&provider_id, &app_type)
                .await?;
            ok(Value::Null)
        }
        "get_circuit_breaker_config" => ok(state
            .app
            .db
            .get_circuit_breaker_config()
            .await
            .map_err(|e| e.to_string())?),
        "update_circuit_breaker_config" => {
            let config = serde_json::from_value(
                arg(&args, "config")
                    .cloned()
                    .ok_or_else(|| "Missing config argument".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            state
                .app
                .db
                .update_circuit_breaker_config(&config)
                .await
                .map_err(|e| e.to_string())?;
            state
                .app
                .proxy_service
                .update_circuit_breaker_configs(config)
                .await?;
            ok(Value::Null)
        }
        "get_failover_queue" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_failover_queue(&app_type)
                .map_err(|e| e.to_string())?)
        }
        "get_available_providers_for_failover" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_available_providers_for_failover(&app_type)
                .map_err(|e| e.to_string())?)
        }
        "add_to_failover_queue" => {
            let app_type = string_arg(&args, "appType")?;
            let provider_id = string_arg(&args, "providerId")?;
            state
                .app
                .db
                .add_to_failover_queue(&app_type, &provider_id)
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "remove_from_failover_queue" => {
            let app_type = string_arg(&args, "appType")?;
            let provider_id = string_arg(&args, "providerId")?;
            state
                .app
                .db
                .remove_from_failover_queue(&app_type, &provider_id)
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "get_auto_failover_enabled" => {
            let app_type = string_arg(&args, "appType")?;
            ok(state
                .app
                .db
                .get_proxy_config_for_app(&app_type)
                .await
                .map_err(|e| e.to_string())?
                .auto_failover_enabled)
        }
        "set_auto_failover_enabled" => {
            let app_type = string_arg(&args, "appType")?;
            let enabled = bool_arg(&args, "enabled")?;
            let mut config = state
                .app
                .db
                .get_proxy_config_for_app(&app_type)
                .await
                .map_err(|e| e.to_string())?;
            config.auto_failover_enabled = enabled;
            state
                .app
                .db
                .update_proxy_config_for_app(config)
                .await
                .map_err(|e| e.to_string())?;
            ok(Value::Null)
        }
        "get_usage_summary" => ok(state
            .app
            .db
            .get_usage_summary(
                optional_i64_arg(&args, "startDate"),
                optional_i64_arg(&args, "endDate"),
                optional_string_arg(&args, "appType").as_deref(),
            )
            .map_err(|e| e.to_string())?),
        "get_usage_trends" => ok(state
            .app
            .db
            .get_daily_trends(
                optional_i64_arg(&args, "startDate"),
                optional_i64_arg(&args, "endDate"),
                optional_string_arg(&args, "appType").as_deref(),
            )
            .map_err(|e| e.to_string())?),
        "get_provider_stats" => ok(state
            .app
            .db
            .get_provider_stats(
                optional_i64_arg(&args, "startDate"),
                optional_i64_arg(&args, "endDate"),
                optional_string_arg(&args, "appType").as_deref(),
            )
            .map_err(|e| e.to_string())?),
        "get_model_stats" => ok(state
            .app
            .db
            .get_model_stats(
                optional_i64_arg(&args, "startDate"),
                optional_i64_arg(&args, "endDate"),
                optional_string_arg(&args, "appType").as_deref(),
            )
            .map_err(|e| e.to_string())?),
        "get_request_logs" => {
            let filters: crate::services::usage_stats::LogFilters = serde_json::from_value(
                arg(&args, "filters").cloned().unwrap_or_else(|| json!({})),
            )
            .map_err(|e| e.to_string())?;
            let page = u32_arg(&args, "page", 0)?;
            let page_size = u32_arg(&args, "pageSize", 20)?;
            ok(state
                .app
                .db
                .get_request_logs(&filters, page, page_size)
                .map_err(|e| e.to_string())?)
        }
        "get_request_detail" => {
            let request_id = string_arg(&args, "requestId")?;
            ok(state
                .app
                .db
                .get_request_detail(&request_id)
                .map_err(|e| e.to_string())?)
        }
        "get_usage_data_sources" => ok(
            crate::services::session_usage::get_data_source_breakdown(state.app.db.as_ref())
                .map_err(|e| e.to_string())?,
        ),
        "set_window_theme" => ok(true),
        "auth_start_login" => {
            let provider = string_arg(&args, "authProvider")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => {
                    let github_domain = optional_string_arg(&args, "githubDomain");
                    let manager = state.copilot.read().await;
                    ok(map_device_code_response(
                        &provider,
                        manager
                            .start_device_flow(github_domain.as_deref())
                            .await
                            .map_err(|e| e.to_string())?,
                    ))
                }
                AUTH_PROVIDER_CODEX_OAUTH => {
                    let manager = state.codex.read().await;
                    ok(map_device_code_response(
                        &provider,
                        manager.start_device_flow().await.map_err(|e| e.to_string())?,
                    ))
                }
                _ => Err(format!("Unsupported auth provider: {provider}")),
            }
        }
        "auth_poll_for_account" => {
            let provider = string_arg(&args, "authProvider")?;
            let device_code = string_arg(&args, "deviceCode")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => {
                    let github_domain = optional_string_arg(&args, "githubDomain");
                    let manager = state.copilot.write().await;
                    match manager
                        .poll_for_token(&device_code, github_domain.as_deref())
                        .await
                    {
                        Ok(account) => {
                            let default = manager.get_status().await.default_account_id;
                            ok(account.map(|account| {
                                map_account(&provider, account, default.as_deref())
                            }))
                        }
                        Err(CopilotAuthError::AuthorizationPending) => ok(Value::Null),
                        Err(e) => Err(e.to_string()),
                    }
                }
                AUTH_PROVIDER_CODEX_OAUTH => {
                    let manager = state.codex.write().await;
                    match manager.poll_for_token(&device_code).await {
                        Ok(account) => {
                            let default = manager.get_status().await.default_account_id;
                            ok(account.map(|account| {
                                map_account(&provider, account, default.as_deref())
                            }))
                        }
                        Err(CodexOAuthError::AuthorizationPending) => ok(Value::Null),
                        Err(e) => Err(e.to_string()),
                    }
                }
                _ => Err(format!("Unsupported auth provider: {provider}")),
            }
        }
        "auth_get_status" => {
            let provider = string_arg(&args, "authProvider")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => {
                    let manager = state.copilot.read().await;
                    let status = manager.get_status().await;
                    let default = status.default_account_id.clone();
                    ok(ManagedAuthStatus {
                        provider: provider.clone(),
                        authenticated: status.authenticated,
                        default_account_id: default.clone(),
                        migration_error: status.migration_error,
                        accounts: status
                            .accounts
                            .into_iter()
                            .map(|account| map_account(&provider, account, default.as_deref()))
                            .collect(),
                    })
                }
                AUTH_PROVIDER_CODEX_OAUTH => {
                    let manager = state.codex.read().await;
                    let status = manager.get_status().await;
                    let default = status.default_account_id.clone();
                    ok(ManagedAuthStatus {
                        provider: provider.clone(),
                        authenticated: status.authenticated,
                        default_account_id: default.clone(),
                        migration_error: None,
                        accounts: status
                            .accounts
                            .into_iter()
                            .map(|account| map_account(&provider, account, default.as_deref()))
                            .collect(),
                    })
                }
                _ => Err(format!("Unsupported auth provider: {provider}")),
            }
        }
        "auth_list_accounts" => {
            let provider = string_arg(&args, "authProvider")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => {
                    let manager = state.copilot.read().await;
                    let status = manager.get_status().await;
                    let default = status.default_account_id.clone();
                    ok(status
                        .accounts
                        .into_iter()
                        .map(|account| map_account(&provider, account, default.as_deref()))
                        .collect::<Vec<_>>())
                }
                AUTH_PROVIDER_CODEX_OAUTH => {
                    let manager = state.codex.read().await;
                    let status = manager.get_status().await;
                    let default = status.default_account_id.clone();
                    ok(status
                        .accounts
                        .into_iter()
                        .map(|account| map_account(&provider, account, default.as_deref()))
                        .collect::<Vec<_>>())
                }
                _ => return Err(format!("Unsupported auth provider: {provider}")),
            }
        }
        "auth_remove_account" => {
            let provider = string_arg(&args, "authProvider")?;
            let account_id = string_arg(&args, "accountId")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => state
                    .copilot
                    .write()
                    .await
                    .remove_account(&account_id)
                    .await
                    .map_err(|e| e.to_string())?,
                AUTH_PROVIDER_CODEX_OAUTH => state
                    .codex
                    .write()
                    .await
                    .remove_account(&account_id)
                    .await
                    .map_err(|e| e.to_string())?,
                _ => return Err(format!("Unsupported auth provider: {provider}")),
            }
            ok(Value::Null)
        }
        "auth_set_default_account" => {
            let provider = string_arg(&args, "authProvider")?;
            let account_id = string_arg(&args, "accountId")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => state
                    .copilot
                    .write()
                    .await
                    .set_default_account(&account_id)
                    .await
                    .map_err(|e| e.to_string())?,
                AUTH_PROVIDER_CODEX_OAUTH => state
                    .codex
                    .write()
                    .await
                    .set_default_account(&account_id)
                    .await
                    .map_err(|e| e.to_string())?,
                _ => return Err(format!("Unsupported auth provider: {provider}")),
            }
            ok(Value::Null)
        }
        "auth_logout" => {
            let provider = string_arg(&args, "authProvider")?;
            match provider.as_str() {
                AUTH_PROVIDER_GITHUB_COPILOT => state
                    .copilot
                    .write()
                    .await
                    .clear_auth()
                    .await
                    .map_err(|e| e.to_string())?,
                AUTH_PROVIDER_CODEX_OAUTH => state
                    .codex
                    .write()
                    .await
                    .clear_auth()
                    .await
                    .map_err(|e| e.to_string())?,
                _ => return Err(format!("Unsupported auth provider: {provider}")),
            }
            ok(Value::Null)
        }
        other => Err(format!("Web RPC command is not implemented yet: {other}")),
    }
}

async fn rpc(
    State(state): State<WebState>,
    Path(command): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let args = body.get("args").cloned().unwrap_or(body);
    match handle_rpc_command(state, &command, args).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
            })),
        )
            .into_response(),
    }
}

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "name": "cc-switch-web"
    }))
}

fn init_state() -> Result<WebState, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    crate::panic_hook::setup_panic_hook();
    crate::settings::reload_settings().map_err(|e| e.to_string())?;
    crate::panic_hook::init_app_config_dir(config::get_app_config_dir());

    let db = Arc::new(Database::init().map_err(|e| e.to_string())?);
    let app = Arc::new(AppState::new(db));
    let data_dir = config::get_app_config_dir();

    Ok(WebState {
        app,
        copilot: Arc::new(RwLock::new(CopilotAuthManager::new(data_dir.clone()))),
        codex: Arc::new(RwLock::new(CodexOAuthManager::new(data_dir))),
    })
}

fn parse_bind_addrs(bind: &str) -> Result<Vec<SocketAddr>, String> {
    let addrs = bind
        .split([',', ';', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<SocketAddr>()
                .map_err(|err| format!("Invalid bind address '{part}': {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if addrs.is_empty() {
        return Err("CC_SWITCH_WEB_BIND did not contain any bind address".to_string());
    }

    Ok(addrs)
}

pub fn run_web() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| {
            eprintln!("Failed to build async runtime: {err}");
            std::process::exit(1);
        });

    runtime.block_on(async move {
        let state = init_state().unwrap_or_else(|err| {
            eprintln!("Failed to initialize CC Switch web server: {err}");
            std::process::exit(1);
        });

        let bind =
            std::env::var("CC_SWITCH_WEB_BIND").unwrap_or_else(|_| "[::]:3650,0.0.0.0:3650".into());
        let addrs = parse_bind_addrs(&bind).unwrap_or_else(|err| {
            eprintln!("Invalid CC_SWITCH_WEB_BIND value '{bind}': {err}");
            std::process::exit(1);
        });

        let dist_dir = std::env::var("CC_SWITCH_WEB_DIST").unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| ".".into())
                .join("dist")
                .to_string_lossy()
                .to_string()
        });

        let app = Router::new()
            .route("/api/health", get(health))
            .route("/api/rpc/:command", post(rpc))
            .fallback_service(ServeDir::new(dist_dir).append_index_html_on_directories(true))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let mut bound = 0usize;
        for addr in addrs {
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => listener,
                Err(err) if err.kind() == io::ErrorKind::AddrInUse && bound > 0 => {
                    eprintln!(
                        "Skipping {addr}: address already in use. This is expected when the IPv6 listener already accepts IPv4."
                    );
                    continue;
                }
                Err(err) => {
                    eprintln!("Failed to bind web server on {addr}: {err}");
                    continue;
                }
            };

            bound += 1;
            println!("CC Switch web server listening on http://{addr}");
            let app = app.clone();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap_or_else(|err| {
                    eprintln!("CC Switch web server stopped on {addr}: {err}");
                    std::process::exit(1);
                });
            });
        }

        if bound == 0 {
            eprintln!("Failed to bind any web server address from CC_SWITCH_WEB_BIND='{bind}'");
            std::process::exit(1);
        }

        futures::future::pending::<()>().await;
    });
}
