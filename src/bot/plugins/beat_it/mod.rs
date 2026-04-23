use crate::bot::plugin::BotPlugin;

#[derive(Clone, Debug, Default)]
pub struct BeatItPlugin;

impl BotPlugin for BeatItPlugin {
    fn plugin_id(&self) -> &'static str {
        "beat-it"
    }
}
