use crate::agent::AgentLoop;
use crate::cli::render;
use crate::config::Settings;
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::SessionStore;
use crate::tool::registry::ToolRegistry;

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    loop {
        let input = render::prompt_user()?;
        if input == ":quit" {
            break;
        }
        if input.is_empty() {
            continue;
        }

        let answer = run_single_prompt(settings.clone(), cwd, input).await?;
        render::print_assistant(&answer);
    }
    Ok(())
}

pub async fn run_single_prompt(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
) -> anyhow::Result<String> {
    let provider = OpenAiCompatibleProvider::new(
        settings.provider.base_url.clone(),
        settings.provider.model.clone(),
        settings.provider.api_key_env.clone(),
    );

    let tool_registry = ToolRegistry::new(&settings, cwd);
    let permissions = PermissionMatcher::new(settings.clone());
    let session = SessionStore::for_workspace(&settings.session.root, cwd)?;

    let loop_runner = AgentLoop {
        provider,
        tool_registry,
        permissions,
        max_steps: settings.agent.max_steps,
        model: settings.provider.model,
        session,
    };

    loop_runner
        .run(prompt, |tool_name| {
            Ok(render::confirm(&format!(
                "Allow tool '{}' execution?",
                tool_name
            ))?)
        })
        .await
}
