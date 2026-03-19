use hh_coding_agent::core::agent::RunnerOutput;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub fn start_simple_agent() -> (mpsc::Sender<String>, Arc<Mutex<Option<RunnerOutput>>>) {
    let (input_tx, mut input_rx) = mpsc::channel(256);
    let output_slot = Arc::new(Mutex::new(None));

    let output_slot_clone = output_slot.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        rt.block_on(async move {
            while let Some(content) = input_rx.recv().await {
                let delta = RunnerOutput::AssistantDelta(format!("Echo: {}", content));
                let msg = RunnerOutput::MessageAdded(hh_coding_agent::Message {
                    role: hh_coding_agent::Role::Assistant,
                    content: format!("Echo: {}", content),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                });
                let mut slot = output_slot_clone.lock().await;
                *slot = Some(delta);
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                let mut slot = output_slot_clone.lock().await;
                *slot = Some(msg);
            }
        });
    });

    (input_tx, output_slot)
}
