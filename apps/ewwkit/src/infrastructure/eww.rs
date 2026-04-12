use crate::domain::{AppState, Presenter};
use async_trait::async_trait;

pub struct EwwPresenter {}

#[async_trait]
impl Presenter for EwwPresenter {
    async fn update_state(&self, state: &AppState) -> anyhow::Result<()> {
        let json = serde_json::to_string(state)?;
        println!("{}", json);
        Ok(())
    }
}
