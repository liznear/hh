use tokio::sync::mpsc;

use super::RunnerOutput;

#[derive(Clone)]
pub struct RunnerOutputChannel {
    tx: mpsc::Sender<RunnerOutput>,
}

impl RunnerOutputChannel {
    pub fn new(tx: mpsc::Sender<RunnerOutput>) -> Self {
        Self { tx }
    }

    pub async fn send(&self, output: RunnerOutput) -> anyhow::Result<()> {
        match self.tx.try_send(output) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(output)) => {
                if is_coalescible_delta(&output) {
                    return Ok(());
                }
                self.tx
                    .send(output)
                    .await
                    .map_err(|_| anyhow::anyhow!("runner output channel closed"))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                anyhow::bail!("runner output channel closed")
            }
        }
    }
}

fn is_coalescible_delta(output: &RunnerOutput) -> bool {
    matches!(
        output,
        RunnerOutput::ThinkingDelta(_) | RunnerOutput::AssistantDelta(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn backpressure_drops_coalescible_deltas_when_channel_is_full() {
        let (tx, mut rx) = mpsc::channel(1);
        let channel = RunnerOutputChannel::new(tx);

        channel
            .send(RunnerOutput::ThinkingDelta("first".to_string()))
            .await
            .expect("send first delta");
        channel
            .send(RunnerOutput::ThinkingDelta("second".to_string()))
            .await
            .expect("drop second delta under pressure");

        let first = rx.recv().await.expect("first message present");
        assert!(matches!(first, RunnerOutput::ThinkingDelta(_)));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn backpressure_keeps_control_events_under_pressure() {
        let (tx, mut rx) = mpsc::channel(1);
        let channel = RunnerOutputChannel::new(tx);

        channel
            .send(RunnerOutput::ThinkingDelta("delta".to_string()))
            .await
            .expect("fill channel");

        let sender = {
            let channel = channel.clone();
            tokio::spawn(async move { channel.send(RunnerOutput::TurnComplete).await })
        };

        let received_first = timeout(Duration::from_millis(50), rx.recv())
            .await
            .expect("first recv should not block")
            .expect("first event exists");
        assert!(matches!(received_first, RunnerOutput::ThinkingDelta(_)));

        sender
            .await
            .expect("sender join")
            .expect("control event send");

        let received_second = rx.recv().await.expect("second event exists");
        assert!(matches!(received_second, RunnerOutput::TurnComplete));
    }
}
