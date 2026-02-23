use crate::core::Message;

#[derive(Debug, Default)]
pub struct AgentState {
    pub messages: Vec<Message>,
    pub step: usize,
}

impl AgentState {
    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
    }
}
