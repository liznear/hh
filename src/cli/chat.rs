use crate::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::cli::render::{self, LiveRender, ThinkingMode};
use crate::config::Settings;
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::SessionStore;
use crate::tool::registry::ToolRegistry;

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    let renderer = LiveRender::new();

    loop {
        let input = render::prompt_user()?;
        if input == ":quit" {
            break;
        }
        if input == ":thinking" {
            let mode = renderer.toggle_thinking_mode();
            let label = match mode {
                ThinkingMode::Collapsed => "collapsed",
                ThinkingMode::Expanded => "expanded",
            };
            render::print_info(&format!("thinking output {}", label));
            continue;
        }
        if input.is_empty() {
            continue;
        }

        renderer.begin_turn();
        run_single_prompt_with_events(settings.clone(), cwd, input, renderer.clone()).await?;
    }
    Ok(())
}

pub async fn run_single_prompt(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
) -> anyhow::Result<String> {
    run_single_prompt_with_events(settings, cwd, prompt, NoopEvents).await
}

pub async fn run_single_prompt_with_events<E>(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
    events: E,
) -> anyhow::Result<String>
where
    E: AgentEvents,
{
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
        events,
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
