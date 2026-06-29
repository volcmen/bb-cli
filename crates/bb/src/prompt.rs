//! The real [`Prompter`] implementation, backed by `inquire`.

use crate::core::{PromptError, Prompter};

/// Interactive prompts via `inquire`.
pub struct InquirePrompter;

fn map_err(e: inquire::InquireError) -> PromptError {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            PromptError::Cancelled
        }
        other => PromptError::Other(other.to_string()),
    }
}

impl Prompter for InquirePrompter {
    fn confirm(&self, message: &str, default: bool) -> Result<bool, PromptError> {
        inquire::Confirm::new(message)
            .with_default(default)
            .prompt()
            .map_err(map_err)
    }

    fn input(&self, message: &str, default: Option<&str>) -> Result<String, PromptError> {
        let mut text = inquire::Text::new(message);
        if let Some(d) = default {
            text = text.with_default(d);
        }
        text.prompt().map_err(map_err)
    }

    fn password(&self, message: &str) -> Result<String, PromptError> {
        inquire::Password::new(message)
            .without_confirmation()
            .prompt()
            .map_err(map_err)
    }

    fn select(&self, message: &str, options: &[String]) -> Result<usize, PromptError> {
        // `raw_prompt` returns the chosen `ListOption`, whose `index` is the
        // authoritative position — no fragile value lookup that could silently
        // resolve to the wrong entry when options contain duplicates.
        inquire::Select::new(message, options.to_vec())
            .raw_prompt()
            .map(|choice| choice.index)
            .map_err(map_err)
    }

    fn editor(&self, message: &str, initial: &str) -> Result<String, PromptError> {
        inquire::Editor::new(message)
            .with_predefined_text(initial)
            .prompt()
            .map_err(map_err)
    }
}
