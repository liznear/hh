use crate::config::Settings;

use crate::app::chat_state::ModelOptionView;

pub(crate) fn build_session_name(_cwd: &std::path::Path) -> String {
    "New Session".to_string()
}

pub(crate) fn format_modalities(
    modalities: &[crate::config::settings::ModelModalityType],
) -> String {
    modalities
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn build_model_options(settings: &Settings) -> Vec<ModelOptionView> {
    settings
        .model_refs()
        .into_iter()
        .filter_map(|model_ref| {
            settings
                .resolve_model_ref(&model_ref)
                .map(|resolved| ModelOptionView {
                    full_id: model_ref,
                    provider_name: if resolved.provider.display_name.trim().is_empty() {
                        resolved.provider_id.clone()
                    } else {
                        resolved.provider.display_name.clone()
                    },
                    model_name: if resolved.model.display_name.trim().is_empty() {
                        resolved.model_id.clone()
                    } else {
                        resolved.model.display_name.clone()
                    },
                    modality: format!(
                        "{} -> {}",
                        format_modalities(&resolved.model.modalities.input),
                        format_modalities(&resolved.model.modalities.output)
                    ),
                    max_context_size: resolved.model.limits.context,
                })
        })
        .collect()
}
