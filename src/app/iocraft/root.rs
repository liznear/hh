use crate::app::events::{ScopedTuiEvent, TuiEventSender};
use crate::app::handlers;
use crate::app::state::{App as MvuApp, AppState};
use crate::app::utils;
use crate::cli::agent_init;
use crate::config::Settings;
use iocraft::prelude::*;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct IocraftAppRunner {
    pub mvu_app: Arc<Mutex<MvuApp>>,
    pub settings: Settings,
    pub cwd: PathBuf,
    pub event_sender: TuiEventSender,
    pub event_rx: Option<mpsc::UnboundedReceiver<ScopedTuiEvent>>,
}

pub async fn run_iocraft_app(settings: Settings, cwd: std::path::PathBuf) -> anyhow::Result<()> {
    let mut app = AppState::new(cwd.clone());
    app.configure_models(
        settings.selected_model_ref().to_string(),
        utils::build_model_options(&settings),
    );

    let (agent_views, selected_agent) = agent_init::initialize_agents(&settings)?;
    app.set_agents(agent_views, selected_agent);

    let (event_tx, event_rx) = mpsc::unbounded_channel::<ScopedTuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);
    handlers::subagent::initialize_subagent_manager(settings.clone(), cwd.clone());

    let mvu_app = Arc::new(Mutex::new(MvuApp::new(app)));

    let runner = Arc::new(Mutex::new(IocraftAppRunner {
        mvu_app: mvu_app.clone(),
        settings,
        cwd,
        event_sender,
        event_rx: Some(event_rx),
    }));

    // TODO: spawn a task to listen to event_rx and call some mechanism to trigger iocraft redraw?

    element!(
        ContextProvider(value: Context::owned(runner)) {
            super::layout::AppRoot
        }
    )
    .fullscreen()
    .await?;

    Ok(())
}
